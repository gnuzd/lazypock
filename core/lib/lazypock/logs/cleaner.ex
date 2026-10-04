defmodule Lazypock.Logs.Cleaner do
  @moduledoc """
  Periodically deletes request-log entries older than the configured retention
  (`Lazypock.Logs.retention_days/0`).

  Runs on a coarse timer (default 1 h, `LAZYPOCK_LOGS_CLEANER_INTERVAL_MS` to
  override) and is a no-op while auto-clean is off, so enabling the setting in
  the Studio takes effect within one interval. Deletion is a single indexed
  `DELETE`, so the pass is cheap.

  As with `Lazypock.Files.Reaper`, `config :lazypock, __MODULE__, async: false`
  (set in `config/test.exs`) keeps the timer from arming under test; tests call
  `run/0` (or `Lazypock.Logs.clean_old/0`) directly.
  """

  use GenServer
  require Logger

  alias Lazypock.Logs

  @default_interval_ms 60 * 60 * 1000

  def start_link(opts \\ []) do
    {name, opts} = Keyword.pop(opts, :name, __MODULE__)
    GenServer.start_link(__MODULE__, opts, name: name)
  end

  @doc "Runs one cleanup pass and returns the number of deleted entries."
  @spec run() :: non_neg_integer()
  def run, do: Logs.clean_old()

  @impl true
  def init(opts) do
    interval = Keyword.get(opts, :interval_ms) || interval_ms()

    if Keyword.get(opts, :auto, true) and async?() do
      send(self(), :tick)
    end

    {:ok, %{interval: interval}}
  end

  @impl true
  def handle_info(:tick, state) do
    Process.send_after(self(), :tick, state.interval)
    safe_run()
    {:noreply, state}
  end

  def handle_info(_message, state), do: {:noreply, state}

  defp safe_run do
    Logs.clean_old()
  rescue
    e -> Logger.warning("Request-log cleanup failed: #{Exception.message(e)}")
  end

  defp async?, do: Keyword.get(Application.get_env(:lazypock, __MODULE__, []), :async, true)

  defp interval_ms do
    case System.get_env("LAZYPOCK_LOGS_CLEANER_INTERVAL_MS") do
      nil ->
        @default_interval_ms

      value ->
        case Integer.parse(value) do
          {ms, _} when ms > 0 -> ms
          _ -> @default_interval_ms
        end
    end
  end
end
