defmodule Lazypock.Auth.RateLimiter do
  @moduledoc """
  ETS-based rate limiter for login attempts.

  Tracks attempts by `{ip, collection, email}` key.
  Max 10 failed attempts per 15-minute sliding window.
  Uses lazy cleanup — expired entries are pruned during normal lookups.

  ## ETS Structure

  Each entry is `{key, count, first_attempt_timestamp}` where:
  - `key` = `{ip, collection, email}` tuple
  - `count` = number of failed attempts in the current window
  - `first_attempt_timestamp` = monotonic time of the first failed attempt

  ## Configuration

      config :lazypock, Lazypock.Auth.RateLimiter,
        max_attempts: 10,
        window_seconds: 900  # 15 minutes
  """

  @table_name :lazypock_rate_limits

  @doc """
  Ensures the ETS table exists. Idempotent — safe to call before every access.
  """
  def ensure_table do
    case :ets.info(@table_name) do
      :undefined ->
        :ets.new(@table_name, [
          :named_table,
          :set,
          :public,
          read_concurrency: true,
          write_concurrency: true
        ])

      _ ->
        :ok
    end
  end

  @doc """
  Checks whether the given identity is rate-limited.

  Returns `:ok` if allowed, `{:error, :rate_limited}` if the limit is exceeded.
  Prunes expired entries during lookup.
  """
  @spec check_rate(String.t(), String.t(), String.t()) :: :ok | {:error, :rate_limited}
  def check_rate(ip, collection, email) do
    ensure_table()

    key = {ip, collection, email}
    now = System.monotonic_time(:second)
    window = window_seconds()
    max = max_attempts()

    case :ets.lookup(@table_name, key) do
      [{^key, count, first_attempt}] ->
        if now - first_attempt > window do
          :ets.delete(@table_name, key)
          :ok
        else
          if count >= max, do: {:error, :rate_limited}, else: :ok
        end

      [] ->
        :ok
    end
  end

  @doc """
  Records a login attempt (success or failure).

  - `:success` — clears the rate limit entry for this key.
  - `:failure` — atomically increments the attempt counter or creates a new entry.
  """
  @spec record_attempt(String.t(), String.t(), String.t(), :success | :failure) :: :ok
  def record_attempt(ip, collection, email, :success) do
    ensure_table()
    :ets.delete(@table_name, {ip, collection, email})
    :ok
  end

  def record_attempt(ip, collection, email, :failure) do
    ensure_table()

    key = {ip, collection, email}
    now = System.monotonic_time(:second)

    # Atomically increment position 2 (count), default to {key, 0, now}
    _new_count = :ets.update_counter(@table_name, key, {2, 1}, {key, 0, now})
    :ok
  end

  @doc """
  Returns the client IP as a string from a Plug.Conn.
  """
  @spec ip_from_conn(Plug.Conn.t()) :: String.t()
  def ip_from_conn(conn) do
    conn.remote_ip
    |> :inet.ntoa()
    |> List.to_string()
  end

  # --- Configuration defaults ---

  defp max_attempts do
    Application.get_env(:lazypock, __MODULE__, [])
    |> Keyword.get(:max_attempts, 10)
  end

  defp window_seconds do
    Application.get_env(:lazypock, __MODULE__, [])
    |> Keyword.get(:window_seconds, 900)
  end
end
