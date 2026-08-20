defmodule Lazypock.Cron.Scheduler do
  @moduledoc """
  Schedules persisted cron jobs (`Lazypock.Cron`) for execution.

  On boot and on every reload the scheduler loads enabled jobs from the
  `_crons` table, computes each job's next run time (in the job's timezone)
  and arms a `Process.send_after` timer per job. When a timer fires, the job
  executes in a spawned `Task` via `Lazypock.Cron.Runner` so a slow job never
  blocks the scheduler or its siblings.

  Reloads are triggered by the admin API after any create/update/delete and
  by a periodic resync (default 30s) which catches out-of-band edits (e.g.
  via the SQL console). Jobs whose stored `next_run_at` is already in the
  past (missed while the node was down, or edited out-of-band) are scheduled
  to run shortly after the next reload.

  DB access is wrapped so a scheduler that is running inside a test sandbox
  (or with a temporarily unavailable database) never crashes the process —
  it simply skips re-arming until the next reload/resync.
  """

  use GenServer

  alias Lazypock.Cron
  alias Lazypock.Cron.Runner

  @resync_interval 30_000
  @missed_run_delay 2_000

  # ── Public API ─────────────────────────────────────

  def start_link(opts \\ []) do
    GenServer.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @doc "Reloads all jobs from the DB and re-arms timers (no-op when not running)."
  def reload do
    if Process.whereis(__MODULE__), do: GenServer.cast(__MODULE__, :reload)
    :ok
  end

  # ── GenServer callbacks ────────────────────────────

  @impl true
  def init(_opts) do
    {:ok, %{jobs: %{}}, {:continue, :reload}}
  end

  @impl true
  def handle_continue(:reload, state) do
    schedule_resync()
    {:noreply, do_reload(state)}
  end

  @impl true
  def handle_cast(:reload, state) do
    {:noreply, do_reload(state)}
  end

  @impl true
  def handle_info(:resync, state) do
    schedule_resync()
    {:noreply, do_reload(state)}
  end

  def handle_info({:tick, job_id}, state) do
    case Map.get(state.jobs, job_id) do
      %{job: job} ->
        # Fire and forget: the task records the outcome and re-arms the timer.
        _ = Task.start(fn -> execute_job(job) end)
        {:noreply, state}

      nil ->
        {:noreply, state}
    end
  end

  # ── Internals ──────────────────────────────────────

  # Cancel all timers and re-arm from the current DB state.
  defp do_reload(state) do
    Enum.each(state.jobs, fn {_id, %{timer: ref}} -> Process.cancel_timer(ref) end)

    jobs = safe_list_jobs()

    new_jobs =
      Map.new(jobs, fn job ->
        case next_run(job) do
          nil ->
            {job["id"], %{job: job, timer: nil}}

          next ->
            delay = max(0, DateTime.diff(next, DateTime.utc_now(), :millisecond))
            timer = Process.send_after(self(), {:tick, job["id"]}, delay)
            {job["id"], %{job: job, timer: timer, next: next}}
        end
      end)

    %{state | jobs: new_jobs}
  end

  # The stored next_run_at wins when it is still in the future (keeps a steady
  # cadence across reloads); otherwise compute a fresh one. Jobs whose run time
  # passed while we were down are re-armed with a short delay.
  defp next_run(job) do
    now = DateTime.utc_now()

    next =
      case job["next_run_at"] do
        %DateTime{} = stored ->
          if DateTime.compare(stored, now) == :gt do
            stored
          else
            Cron.next_run_at(job["expression"], job["timezone"], now)
          end

        _ ->
          Cron.next_run_at(job["expression"], job["timezone"], now)
      end

    case next do
      %DateTime{} = dt ->
        if DateTime.compare(dt, now) == :gt do
          dt
        else
          DateTime.add(now, @missed_run_delay, :millisecond)
        end

      nil ->
        nil
    end
  end

  defp execute_job(job) do
    started = System.monotonic_time(:millisecond)
    result = Runner.run(job)
    duration_ms = System.monotonic_time(:millisecond) - started
    _updated = Cron.record_run!(job, result, duration_ms)
    # Re-arm this job (and any others) from the updated DB state.
    reload()
  end

  # Never crash the scheduler on DB hiccups (test sandbox, transient outage):
  # an empty job set just means no timers are armed until the next resync.
  defp safe_list_jobs do
    try do
      Cron.list()
    rescue
      _ -> []
    catch
      _, _ -> []
    end
  end

  defp schedule_resync do
    Process.send_after(self(), :resync, @resync_interval)
  end
end
