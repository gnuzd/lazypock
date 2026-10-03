defmodule Lazypock.Files.Limiter do
  @moduledoc """
  Bounds how many image jobs (ImageMagick) run at the same time in this BEAM.

  Without a limit a burst of uploads or `/scale` requests spawns one `magick`
  process per request. Every ImageMagick process easily allocates 50–150 MB of
  RSS for a 12 MP image, so a handful of concurrent resizes OOMs a small VPS.
  This serialises the expensive work behind a fixed number of slots.

  ## Behaviour

    * `run/2` executes the given function while holding one slot.
    * When all slots are busy the caller waits, up to `:wait` milliseconds
      (default 5 s), then gets `{:error, :overloaded}` — the HTTP layer maps
      that to `503` + `Retry-After` instead of hanging.
    * The wait queue itself is bounded (16 × `limit`); beyond that callers fail
      fast instead of piling up unbounded work in memory.
    * Waiting callers are monitored, so a disconnected request does not leak a
      queue entry.

  ## Configuration

  Read at boot from the application env or the environment:

      config :lazypock, Lazypock.Files.Limiter, limit: 1, wait: 5_000

      LAZYPOCK_IMAGE_CONCURRENCY=1   # overrides :limit

  The default is **1 slot per instance**, which is the safe choice on a 2 GB VPS.
  """

  use GenServer

  @default_limit 1
  @default_wait 5_000
  @queue_factor 16

  # ── Public API ───────────────────────────────────────

  def start_link(opts \\ []) do
    {name, opts} = Keyword.pop(opts, :name, __MODULE__)
    GenServer.start_link(__MODULE__, opts, name: name)
  end

  @doc """
  Runs `fun` while holding one slot.

  Returns `{:ok, fun.()}` or `{:error, :overloaded}` when no slot became free
  within the wait budget.

  ## Options

    * `:server` — the limiter instance (default `#{inspect(__MODULE__)}`)
    * `:wait` — milliseconds to wait for a slot (default `#{@default_wait}`)
  """
  @spec run((-> result), keyword()) :: {:ok, result} | {:error, :overloaded} when result: term()
  def run(fun, opts \\ []) when is_function(fun, 0) do
    server = Keyword.get(opts, :server, __MODULE__)
    wait = Keyword.get(opts, :wait, default_wait())

    case acquire(server, wait) do
      :ok ->
        try do
          {:ok, fun.()}
        after
          release(server)
        end

      {:error, :overloaded} = error ->
        error
    end
  end

  @doc "Current limiter state: `%{limit: n, running: n, queued: n}`."
  def stats(server \\ __MODULE__) do
    GenServer.call(server, :stats)
  catch
    :exit, _ -> %{limit: 0, running: 0, queued: 0}
  end

  @doc "The configured slot count for `server` (default instance when omitted)."
  def limit(server \\ __MODULE__) do
    GenServer.call(server, :limit)
  catch
    :exit, _ -> configured_limit()
  end

  @doc false
  def configured_limit do
    case System.get_env("LAZYPOCK_IMAGE_CONCURRENCY") do
      nil ->
        Keyword.get(config(), :limit, @default_limit) |> normalize_limit()

      value ->
        case Integer.parse(value) do
          {n, _} -> normalize_limit(n)
          :error -> @default_limit
        end
    end
  end

  defp normalize_limit(n) when is_integer(n) and n > 0, do: n
  defp normalize_limit(_), do: @default_limit

  defp default_wait, do: Keyword.get(config(), :wait, @default_wait)

  defp config, do: Application.get_env(:lazypock, __MODULE__, [])

  # ── Slot bookkeeping ─────────────────────────────────

  defp acquire(server, wait) do
    GenServer.call(server, {:acquire, wait}, wait + 1_000)
  catch
    :exit, _ -> {:error, :overloaded}
  end

  defp release(server) do
    GenServer.cast(server, :release)
  end

  # ── GenServer ────────────────────────────────────────

  @impl true
  def init(opts) do
    limit = Keyword.get(opts, :limit) || configured_limit()

    {:ok,
     %{
       limit: normalize_limit(limit),
       running: 0,
       queue: [],
       waiters: %{}
     }}
  end

  @impl true
  def handle_call({:acquire, wait}, from, state) do
    cond do
      state.running < state.limit ->
        {:reply, :ok, %{state | running: state.running + 1}}

      length(state.queue) >= state.limit * @queue_factor ->
        {:reply, {:error, :overloaded}, state}

      true ->
        {:noreply, enqueue(state, from, wait)}
    end
  end

  def handle_call(:stats, _from, state) do
    {:reply, %{limit: state.limit, running: state.running, queued: length(state.queue)}, state}
  end

  def handle_call(:limit, _from, state), do: {:reply, state.limit, state}

  @impl true
  def handle_cast(:release, state) do
    case state.queue do
      [{ref, from, monitor, _timer} | queue] ->
        # Hand the slot straight to the next waiter — `running` stays the same.
        Process.demonitor(monitor, [:flush])
        GenServer.reply(from, :ok)

        {:noreply, %{state | queue: queue, waiters: Map.delete(state.waiters, ref)}}

      [] ->
        {:noreply, %{state | running: max(state.running - 1, 0)}}
    end
  end

  @impl true
  def handle_info({:waiter_timeout, ref}, state) do
    case Map.pop(state.waiters, ref) do
      {nil, _} ->
        {:noreply, state}

      {%{from: from, monitor: monitor}, waiters} ->
        Process.demonitor(monitor, [:flush])
        GenServer.reply(from, {:error, :overloaded})

        {:noreply, %{state | queue: drop_waiter(state.queue, ref), waiters: waiters}}
    end
  end

  def handle_info({:DOWN, monitor, :process, _pid, _reason}, state) do
    case Enum.find(state.waiters, fn {_ref, w} -> w.monitor == monitor end) do
      nil ->
        {:noreply, state}

      {ref, _waiter} ->
        {:noreply,
         %{state | queue: drop_waiter(state.queue, ref), waiters: Map.delete(state.waiters, ref)}}
    end
  end

  def handle_info(_message, state), do: {:noreply, state}

  defp enqueue(state, from, wait) do
    ref = make_ref()
    {pid, _tag} = from
    monitor = Process.monitor(pid)
    timer = Process.send_after(self(), {:waiter_timeout, ref}, wait)

    entry = {ref, from, monitor, timer}

    %{
      state
      | queue: state.queue ++ [entry],
        waiters: Map.put(state.waiters, ref, %{from: from, monitor: monitor, timer: timer})
    }
  end

  # FIFO order matters, so drop by predicate rather than splitting the list.
  defp drop_waiter(queue, ref) do
    Enum.reject(queue, fn {entry_ref, _from, _monitor, timer} ->
      if entry_ref == ref do
        Process.cancel_timer(timer)
        true
      else
        false
      end
    end)
  end
end
