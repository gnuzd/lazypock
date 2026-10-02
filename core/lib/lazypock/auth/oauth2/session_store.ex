defmodule Lazypock.Auth.OAuth2.SessionStore do
  @moduledoc """
  Owns the pending-OAuth2-session ETS table.

  During `GET /api/:collection/auth-methods` the backend generates a PKCE
  `code_verifier`/`state` pair and stores `state → {provider, collection,
  verifier, app_origin}` until the provider redirects back to
  `/api/oauth2-redirect`. Because `auth-methods` is unauthenticated, this store
  is a potential memory-DoS target and a session-tampering target.

  The table is created `:protected` and owned by this GenServer, so only this
  process can write to it. Every other process goes through the API below. A
  periodic sweep plus a hard cap bound memory even under sustained abuse.
  """

  use GenServer

  @table :lazypock_oauth2_sessions
  # Pending sessions are valid for 10 minutes (PocketBase-comparable).
  @default_ttl_ms 10 * 60 * 1000
  # Hard cap. `auth-methods` is unauthenticated, so refuse new sessions rather
  # than grow without bound once the store is full and nothing has expired.
  @default_max_sessions 50_000
  # Delete expired sessions at least this often.
  @sweep_interval_ms 60_000

  def start_link(opts \\ []) do
    GenServer.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @doc """
  Store a pending session.

  Returns `{:ok, state}` or `{:error, :too_many_sessions}`.
  """
  @spec store(String.t(), String.t(), String.t(), String.t(), String.t() | nil) ::
          {:ok, String.t()} | {:error, :too_many_sessions}
  def store(provider, collection, code_verifier, state, app_origin) do
    GenServer.call(__MODULE__, {:store, provider, collection, code_verifier, state, app_origin})
  end

  @doc """
  Consume a pending session by state.

  Returns `{:ok, provider, collection, code_verifier, app_origin}` or
  `{:error, :expired | :not_found}`. The entry is deleted even when expired, so
  a state is single-use.
  """
  @spec take(String.t()) ::
          {:ok, String.t(), String.t(), String.t(), String.t() | nil} | {:error, atom()}
  def take(state) when is_binary(state) and state != "" do
    GenServer.call(__MODULE__, {:take, state})
  end

  def take(_state), do: {:error, :not_found}

  @doc "Delete expired sessions immediately. Returns the number deleted."
  @spec sweep() :: non_neg_integer()
  def sweep, do: GenServer.call(__MODULE__, :sweep)

  @doc "Number of pending sessions currently stored."
  @spec count() :: non_neg_integer()
  def count, do: GenServer.call(__MODULE__, :count)

  # ── GenServer ───────────────────────────────────────────────

  @impl true
  def init(_opts) do
    :ets.new(@table, [:named_table, :set, :protected, read_concurrency: true])
    schedule_sweep()
    {:ok, %{}}
  end

  @impl true
  def handle_call(
        {:store, provider, collection, verifier, state_key, app_origin},
        _from,
        state
      ) do
    reply =
      if size() >= max_sessions() do
        sweep_expired()

        if size() >= max_sessions() do
          {:error, :too_many_sessions}
        else
          insert(state_key, provider, collection, verifier, app_origin)
        end
      else
        insert(state_key, provider, collection, verifier, app_origin)
      end

    {:reply, reply, state}
  end

  def handle_call({:take, state}, _from, state_acc) do
    {:reply, take_entry(state), state_acc}
  end

  def handle_call(:sweep, _from, state) do
    {:reply, sweep_expired(), state}
  end

  def handle_call(:count, _from, state) do
    {:reply, size(), state}
  end

  @impl true
  def handle_info(:sweep, state) do
    sweep_expired()
    schedule_sweep()
    {:noreply, state}
  end

  # ── internals ───────────────────────────────────────────────

  defp insert(state, provider, collection, verifier, app_origin) do
    :ets.insert(@table, {state, provider, collection, verifier, now(), app_origin})
    {:ok, state}
  end

  defp take_entry(state) do
    case :ets.take(@table, state) do
      [{^state, provider, collection, verifier, ts, app_origin}] ->
        if now() - ts <= ttl_ms() do
          {:ok, provider, collection, verifier, app_origin}
        else
          {:error, :expired}
        end

      _ ->
        {:error, :not_found}
    end
  end

  defp sweep_expired do
    cutoff = now() - ttl_ms()

    :ets.select_delete(@table, [
      {{:"$1", :_, :_, :_, :"$2", :_}, [{:<, :"$2", cutoff}], [true]}
    ])
  end

  defp size, do: :ets.info(@table, :size)
  defp now, do: System.system_time(:millisecond)

  defp ttl_ms, do: Application.get_env(:lazypock, :oauth2_session_ttl_ms, @default_ttl_ms)

  defp max_sessions do
    Application.get_env(:lazypock, :oauth2_session_max, @default_max_sessions)
  end

  defp schedule_sweep, do: Process.send_after(self(), :sweep, @sweep_interval_ms)
end
