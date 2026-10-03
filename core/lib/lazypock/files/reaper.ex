defmodule Lazypock.Files.Reaper do
  @moduledoc """
  Deletes stored objects for `_files` rows that no longer exist.

  Every deletion path (API, Studio, record cascade, raw SQL) ends the same way:
  the `_files` row is removed and an `AFTER DELETE` trigger writes an outbox row
  into `_file_deletions` **in the same transaction**. This worker removes the
  objects and the outbox row afterwards.

  Ordering is always **database first, storage second**: a `_files` row therefore
  never points at an object that was deleted early, and a storage outage only
  delays cleanup (the outbox row is retried with exponential backoff).

  The worker is kicked after every delete, at boot, and on a timer
  (`files.reaper_interval`, default 1 h). A short interval keeps Neon's
  scale-to-zero compute awake, so the default is deliberately coarse.
  """

  use GenServer

  require Logger

  alias Lazypock.Repo

  @default_interval_ms 60 * 60 * 1000
  @lease_ms 5 * 60 * 1000
  @max_backoff_ms 24 * 60 * 60 * 1000

  # ── Public API ───────────────────────────────────────

  def start_link(opts \\ []) do
    {name, opts} = Keyword.pop(opts, :name, __MODULE__)
    GenServer.start_link(__MODULE__, opts, name: name)
  end

  @doc "Ask the reaper to run soon (returns immediately)."
  def kick(server \\ __MODULE__) do
    # Under test the reaper is synchronous-only (`async: false`): a background
    # process sharing the SQL sandbox connection would race the test process.
    if async?(), do: GenServer.cast(server, :kick), else: :ok
  end

  @doc """
  Processes up to `limit` pending deletions synchronously.

  Exposed for tests, the `lazypock files reap` CLI and the health check.
  """
  def drain(limit \\ 50) do
    rows = claim(limit)
    Enum.each(rows, &process/1)
    :ok
  end

  @doc "Queue depth and the age (seconds) of the oldest pending entry."
  def stats do
    case Ecto.Adapters.SQL.query(
           Repo,
           "SELECT COUNT(*)::int, COALESCE(EXTRACT(EPOCH FROM (now() - MIN(created_at)))::int, 0) FROM _file_deletions",
           []
         ) do
      {:ok, %{rows: [[count, age]]}} -> %{pending: count, oldest_seconds: age}
      {:error, _} -> %{pending: 0, oldest_seconds: 0}
    end
  rescue
    _ -> %{pending: 0, oldest_seconds: 0}
  end

  # ── GenServer ────────────────────────────────────────

  @impl true
  def init(opts) do
    interval = Keyword.get(opts, :interval_ms) || interval_ms()

    if Keyword.get(opts, :auto, true) and async?() do
      send(self(), :tick)
    end

    {:ok, %{interval: interval}}
  end

  @impl true
  def handle_cast(:kick, state) do
    safe_drain()
    {:noreply, state}
  end

  @impl true
  def handle_info(:tick, state) do
    safe_drain()
    Process.send_after(self(), :tick, state.interval)
    {:noreply, state}
  end

  def handle_info(_message, state), do: {:noreply, state}

  defp safe_drain do
    reap_stale_pending()
    drain()
  rescue
    e -> Logger.warning("File reaper failed: #{Exception.message(e)}")
  end

  @doc """
  Deletes `pending` uploads that were never completed (older than
  `files.pending_ttl_ms`, default 1 h). The `AFTER DELETE` trigger enqueues the
  object for removal, so the bucket is cleaned up too.
  """
  def reap_stale_pending do
    Ecto.Adapters.SQL.query!(
      Repo,
      "DELETE FROM _files WHERE status = 'pending' AND created_at < now() - make_interval(secs => $1)",
      [pending_ttl_seconds()]
    )

    :ok
  end

  # ── Claim + process ──────────────────────────────────

  # Atomic claim-and-lease: one statement, safe across instances.
  defp claim(limit) do
    query = """
    UPDATE _file_deletions d
    SET next_try_at = now() + make_interval(secs => $2)
    WHERE d.id IN (
      SELECT id FROM _file_deletions
      WHERE next_try_at <= now()
      ORDER BY id
      LIMIT $1
      FOR UPDATE SKIP LOCKED
    )
    RETURNING d.id, d.file_id, d.backend, d.storage_path, d.thumbs, d.attempts
    """

    case Ecto.Adapters.SQL.query(Repo, query, [limit, div(@lease_ms, 1000)]) do
      {:ok, %{rows: rows, columns: cols}} ->
        Enum.map(rows, fn row -> cols |> Enum.zip(row) |> Map.new() end)

      {:error, reason} ->
        Logger.warning("File reaper could not claim work: #{inspect(reason)}")
        []
    end
  end

  defp process(row) do
    case delete_objects(row) do
      :ok ->
        Ecto.Adapters.SQL.query!(Repo, "DELETE FROM _file_deletions WHERE id = $1", [row["id"]])

      {:error, reason} ->
        attempts = (row["attempts"] || 0) + 1

        Logger.warning(
          "File reaper could not delete #{row["storage_path"]} (attempt #{attempts}): #{inspect(reason)}"
        )

        Ecto.Adapters.SQL.query!(
          Repo,
          "UPDATE _file_deletions SET attempts = $2, next_try_at = now() + make_interval(secs => $3) WHERE id = $1",
          [row["id"], attempts, backoff_seconds(attempts)]
        )
    end
  end

  defp delete_objects(row) do
    record = %{
      "id" => uuid_to_string(row["file_id"]),
      "storage_backend" => row["backend"],
      "storage_path" => row["storage_path"],
      "thumbs" => decode_thumbs(row["thumbs"])
    }

    mod = Lazypock.Files.Adapter.for_backend(row["backend"])

    case mod.delete(record) do
      :ok -> :ok
      {:error, reason} -> {:error, reason}
    end
  rescue
    e -> {:error, e}
  end

  # Postgrex returns uuid columns as raw 16-byte binaries.
  defp uuid_to_string(uuid) when is_binary(uuid) and byte_size(uuid) == 16 do
    case Ecto.UUID.load(uuid) do
      {:ok, str} -> str
      :error -> Base.encode16(uuid, case: :lower)
    end
  end

  defp uuid_to_string(uuid), do: to_string(uuid)

  defp decode_thumbs(thumbs) when is_map(thumbs), do: thumbs

  defp decode_thumbs(thumbs) when is_binary(thumbs) do
    case Jason.decode(thumbs) do
      {:ok, map} -> map
      _ -> %{}
    end
  end

  defp decode_thumbs(_), do: %{}

  # 2^attempts seconds, capped at 24 h.
  defp backoff_seconds(attempts) do
    trunc(:math.pow(2, attempts)) |> min(div(@max_backoff_ms, 1000))
  end

  defp pending_ttl_seconds do
    case System.get_env("LAZYPOCK_PENDING_UPLOAD_TTL_MS") do
      nil ->
        case Lazypock.Settings.get("files", %{}) do
          %{"pending_ttl_ms" => ms} when is_integer(ms) and ms > 0 -> div(ms, 1000)
          _ -> 3600
        end

      value ->
        case Integer.parse(value) do
          {ms, _} when ms > 0 -> div(ms, 1000)
          _ -> 3600
        end
    end
  rescue
    _ -> 3600
  end

  defp config, do: Application.get_env(:lazypock, __MODULE__, [])

  defp async?, do: Keyword.get(config(), :async, true)

  defp interval_ms do
    case System.get_env("LAZYPOCK_REAPER_INTERVAL_MS") do
      nil ->
        case Lazypock.Settings.get("files", %{}) do
          %{"reaper_interval_ms" => ms} when is_integer(ms) and ms > 0 -> ms
          _ -> @default_interval_ms
        end

      value ->
        case Integer.parse(value) do
          {ms, _} when ms > 0 -> ms
          _ -> @default_interval_ms
        end
    end
  rescue
    _ -> @default_interval_ms
  end
end
