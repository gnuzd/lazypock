defmodule Lazypock.Backup do
  @moduledoc """
  Full-database export / import (backup & restore) for LazyPock.

  Both the Studio (Settings → Backups / Import) and the CLI
  (`lazypock backup`, `lazypock restore <file>`) go through this module, so
  every restore path shares the same normalization and record-handling.

  ## Export

  `export/0` returns `%{collections: [...]}` where each collection carries
  its `id`, `name`, `type`, `schema`, `rules`, `options`, `hooks` and all
  `records`. This is the format the Studio's "Download Backup" writes and
  the format `lazypock restore` reads.

  ## Import / restore

  `restore/2` accepts either the export envelope (`%{"collections" => [...]}`)
  or a bare collection list, in both the LazyPock backup format (`"schema"`
  key) and the PocketBase 23+ export format (`"fields"` key, camelCase field
  names, PB collection ids — see `normalize_import_payload/1`).

  System-managed fields (`"system": true` in the payload — e.g. PocketBase's
  own `id` primary key field) are stripped before a collection is created or
  updated: LazyPock's DDL layer already generates its own `id` column, so a
  payload that re-declares one explicitly would otherwise collide with it.

  Records are restored with `Lazypock.Schemas.GenericRecord.restore/2`, an
  **upsert by id**: ids (and timestamps) are preserved so relations between
  records survive, and restoring the same backup twice never duplicates
  rows.
  """

  require Logger

  import Ecto.Query

  alias Lazypock.Repo
  alias Lazypock.Schemas.GenericRecord

  # NDJSON archive format (see export_stream/1). `format_version` is written into
  # every manifest so a future format change is detectable by the importer.
  @archive_format "lazypock-archive"
  @archive_format_version 1

  @snapshot_table "_import_snapshots"
  # Keep a short history so a repeated import can still be undone.
  @kept_snapshots 5

  # Cursor batch size: upper bound on rows held in memory while streaming.
  @default_cursor_rows 500
  # Default DB size above which the automatic undo checkpoint is skipped and an
  # explicit confirmation is required instead (LAZYPOCK_IMPORT_UNDO_MAX_MB).
  @undo_max_mb_default 1024

  @doc """
  Exports every collection (schema, rules, options, hooks) plus all records.

  Returns the backup payload as `%{collections: [...]}` — pass it straight
  to `Jason.encode!/1` to write a backup file.
  """
  @spec export() :: %{collections: list(map())}
  def export do
    # Skip collections whose backing table is gone (stale `_collections` row) —
    # querying one would error and, inside a transaction, abort the whole
    # batch (25P02).
    tables = existing_tables()

    collections =
      all_collections()
      |> Enum.filter(fn coll -> is_nil(tables) or MapSet.member?(tables, coll.name) end)
      |> Enum.map(fn coll ->
        Map.put(collection_def(coll), :records, GenericRecord.all(coll.name))
      end)

    %{collections: collections}
  end

  # ── Streaming NDJSON archive export ──────────────────────────────────────

  @doc """
  Streams a full backup to an NDJSON archive without ever materializing more
  than one cursor batch of rows at a time.

  Archive layout:

  ```text
  manifest.json          # format + version + per-collection row counts
  schema.json            # { "collections": [ ... ] } — no records
  data/<name>.ndjson     # one JSON object per line, one line per record
  ```

  Every collection is read from **one `REPEATABLE READ` snapshot** (so a write
  during the export cannot produce a torn backup) using a server-side cursor.
  Each row is encoded and appended on its own, which is what makes a single
  >100 MB column value tractable: no buffer holds more than one row.

  ## Options

    * `:dest` — output path; defaults to `<backup_dir>/lazypock-<kind>-<ts>.zip`
    * `:kind` — `"backup"` (default) or `"checkpoint"`, for the default name/manifest
    * `:exclude_system` — skip system collections (undo checkpoints do this)
    * `:max_rows` — cursor batch size (default #{@default_cursor_rows})

  Returns `{:ok, %{path:, bytes:, collections:, records:, collection_counts:}}`
  or `{:error, reason}`. `GET /api/export`'s JSON shape is untouched by this.
  """
  @spec export_stream(keyword()) :: {:ok, map()} | {:error, term()}
  def export_stream(opts \\ []) do
    dest = Keyword.get_lazy(opts, :dest, fn -> default_archive_path(opts) end)
    kind = Keyword.get(opts, :kind, "backup")
    scratch = scratch_dir("export")

    try do
      File.mkdir_p!(Path.join(scratch, "data"))
      File.mkdir_p!(Path.dirname(dest))

      {schema_collections, counts} = write_archive_data(scratch, opts)
      totals = Enum.reduce(counts, 0, fn c, acc -> acc + c["records"] end)

      write_json!(Path.join(scratch, "schema.json"), %{"collections" => schema_collections})

      write_json!(Path.join(scratch, "manifest.json"), %{
        "format" => @archive_format,
        "format_version" => @archive_format_version,
        "lazypock_version" => lazypock_version(),
        "kind" => kind,
        "created_at" => DateTime.utc_now() |> DateTime.to_iso8601(),
        "collections" => counts,
        "totals" => %{"collections" => length(counts), "records" => totals}
      })

      zip_scratch(scratch, dest)

      {:ok,
       %{
         path: dest,
         bytes: File.stat!(dest).size,
         collections: length(counts),
         records: totals,
         collection_counts: counts
       }}
    rescue
      e -> {:error, Exception.message(e)}
    after
      File.rm_rf(scratch)
    end
  end

  # Reads every collection inside ONE snapshot and writes each collection's
  # NDJSON file. Returns {schema_collections, counts}.
  defp write_archive_data(scratch, opts) do
    max_rows = Keyword.get(opts, :max_rows, @default_cursor_rows)
    exclude_system = Keyword.get(opts, :exclude_system, false)
    tables = existing_tables()

    collections =
      all_collections()
      |> Enum.filter(fn coll -> is_nil(tables) or MapSet.member?(tables, coll.name) end)
      |> Enum.reject(fn coll ->
        exclude_system and Lazypock.Collections.Collection.system?(coll.name)
      end)

    # `REPEATABLE READ` is snapshot isolation: it does not block writers, so a
    # long export does not stall the live app. It does hold dead tuples back
    # from autovacuum until the transaction ends (temporary bloat) — documented
    # tradeoff for a consistent backup.
    # `SET TRANSACTION ISOLATION LEVEL` is only legal before any other statement
    # in the transaction, and issuing it in an already-active transaction both
    # fails AND aborts that transaction. So it is only issued when this export
    # actually owns a fresh transaction.
    #
    # Two cases where it must be skipped:
    #   * the caller is already inside `Repo.transaction/2`; and
    #   * the repo runs on the SQL sandbox, whose per-test transaction is opened at
    #     the connection level, so `Repo.in_transaction?/0` is false even though a
    #     transaction is active.
    # In both, the export proceeds at the ambient isolation level: the point-in-time
    # guarantee is unavailable, not silently broken.
    fresh_transaction? = not Repo.in_transaction?() and not sandbox_pool?()

    counts =
      case Repo.transaction(fn ->
             if fresh_transaction? do
               Ecto.Adapters.SQL.query!(Repo, "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
             end

             Enum.map(collections, fn coll ->
               count = write_collection_ndjson(scratch, coll, max_rows)
               %{"name" => coll.name, "type" => coll.type, "records" => count}
             end)
           end) do
        {:ok, counts} -> counts
        {:error, reason} -> raise "export failed: #{inspect(reason)}"
      end

    {Enum.map(collections, &collection_def/1), counts}
  end

  defp sandbox_pool? do
    :lazypock
    |> Application.get_env(Lazypock.Repo, [])
    |> Keyword.get(:pool)
    |> Kernel.==(Ecto.Adapters.SQL.Sandbox)
  end

  # View collections are read-only and `restore/3` never re-inserts their rows,
  # so they are represented in schema.json only.
  defp write_collection_ndjson(_scratch, %{type: "view"}, _max_rows), do: 0

  defp write_collection_ndjson(scratch, coll, max_rows) do
    path = ndjson_path(scratch, coll.name)

    File.open!(path, [:write, :raw, :binary], fn io ->
      coll.name
      |> GenericRecord.stream_all(max_rows: max_rows)
      |> Enum.reduce(0, fn record, count ->
        IO.binwrite(io, [Jason.encode!(record), "\n"])
        count + 1
      end)
    end)
  end

  defp collection_def(coll) do
    %{
      id: coll.id,
      name: coll.name,
      type: coll.type,
      schema: coll.schema,
      rules: coll.rules,
      options: coll.options,
      hooks: coll.hooks
    }
  end

  @doc "Path of a collection's NDJSON file inside an unpacked archive."
  def ndjson_path(root, collection_name),
    do: Path.join([root, "data", collection_name <> ".ndjson"])

  defp zip_scratch(scratch, dest) do
    data_dir = Path.join(scratch, "data")

    entries =
      ["manifest.json", "schema.json"] ++
        Enum.map(File.ls!(data_dir) |> Enum.sort(), &"data/#{&1}")

    case :zip.create(
           String.to_charlist(dest),
           Enum.map(entries, &String.to_charlist/1),
           [{:cwd, String.to_charlist(scratch)}]
         ) do
      {:ok, _} -> :ok
      {:error, reason} -> raise "could not create archive: #{inspect(reason)}"
    end
  end

  defp write_json!(path, data), do: File.write!(path, Jason.encode!(data, pretty: true))

  defp scratch_dir(prefix) do
    Path.join(System.tmp_dir!(), "lazypock-#{prefix}-#{System.unique_integer([:positive])}")
  end

  defp default_archive_path(opts) do
    kind = Keyword.get(opts, :kind, "backup")
    ts = DateTime.utc_now() |> DateTime.to_iso8601() |> String.replace(~r/[:.]/, "-")
    Path.join(backup_dir(), "lazypock-#{kind}-#{ts}.zip")
  end

  @doc """
  Path for a throwaway export archive used by the HTTP download route.

  The file is deliberately left on disk: `send_file/3` streams it asynchronously,
  so there is no safe point to unlink it here. `sweep_temp_archives/0` runs
  before each new one so they cannot accumulate.
  """
  @spec temp_archive_path() :: String.t()
  def temp_archive_path do
    sweep_temp_archives()
    Path.join(System.tmp_dir!(), "lazypock-export-#{System.unique_integer([:positive])}.zip")
  end

  @doc "Deletes temporary export archives older than one hour (best effort)."
  @spec sweep_temp_archives() :: :ok
  def sweep_temp_archives do
    cutoff = System.system_time(:second) - 3600

    System.tmp_dir!()
    |> Path.join("lazypock-export-*.zip")
    |> Path.wildcard()
    |> Enum.each(fn path ->
      case File.stat(path, time: :posix) do
        {:ok, %{mtime: mtime}} when mtime < cutoff -> File.rm(path)
        _ -> :ok
      end
    end)

    :ok
  end

  defp lazypock_version do
    case Application.spec(:lazypock, :vsn) do
      nil -> "unknown"
      vsn -> List.to_string(vsn)
    end
  end

  # ── Backup location + undo policy configuration ──────────────────────────

  @doc """
  Directory backups and undo checkpoints are written to.

  `LAZYPOCK_BACKUP_DIR` wins; otherwise `<priv>/backups`. Deliberately NOT the
  Files upload root: backups must not be lost on redeploy and must not turn
  into `_files` rows served by `GET /api/files/:id`.
  """
  @spec backup_dir() :: String.t()
  def backup_dir do
    System.get_env("LAZYPOCK_BACKUP_DIR") ||
      Application.get_env(:lazypock, :backup_dir) ||
      case :code.priv_dir(:lazypock) do
        {:error, _} -> Path.join(File.cwd!(), "priv/backups")
        priv -> Path.join(List.to_string(priv), "backups")
      end
  end

  @doc "Total on-disk size of the user collections (what a checkpoint copies)."
  @spec db_size_bytes() :: non_neg_integer()
  def db_size_bytes do
    names =
      all_collections()
      |> Enum.reject(&Lazypock.Collections.Collection.system?(&1.name))
      |> Enum.map(& &1.name)

    case names do
      [] ->
        0

      _ ->
        sql = """
        SELECT COALESCE(SUM(pg_total_relation_size(c.oid)), 0)::bigint
        FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = ANY (current_schemas(false))
          AND c.relname = ANY ($1)
        """

        case Ecto.Adapters.SQL.query(Repo, sql, [names]) do
          {:ok, %{rows: [[bytes]]}} when is_integer(bytes) -> bytes
          _ -> 0
        end
    end
  end

  @doc "`LAZYPOCK_IMPORT_UNDO_MAX_MB` (default #{@undo_max_mb_default}). 0 disables the checkpoint entirely."
  @spec undo_max_mb() :: non_neg_integer()
  def undo_max_mb do
    case Integer.parse(System.get_env("LAZYPOCK_IMPORT_UNDO_MAX_MB") || "") do
      {n, _} when n >= 0 -> n
      _ -> @undo_max_mb_default
    end
  end

  @spec undo_threshold_bytes() :: non_neg_integer()
  def undo_threshold_bytes, do: undo_max_mb() * 1_048_576

  @doc "How many undo checkpoints to keep (`LAZYPOCK_IMPORT_UNDO_KEEP`, default #{@kept_snapshots})."
  @spec undo_keep() :: pos_integer()
  def undo_keep do
    case Integer.parse(System.get_env("LAZYPOCK_IMPORT_UNDO_KEEP") || "") do
      {n, _} when n > 0 -> n
      _ -> @kept_snapshots
    end
  end

  @doc "True while the DB is small enough to take an automatic undo checkpoint."
  @spec undo_available?() :: boolean()
  def undo_available?, do: db_size_bytes() <= undo_threshold_bytes()

  @doc """
  Everything a UI/CLI needs to warn *before* a large import is attempted.

  ```elixir
  %{db_size_bytes: _, db_size_mb: _, threshold_mb: _, undo_available: _, neon_hosted: _}
  ```
  """
  @spec preflight() :: map()
  def preflight do
    bytes = db_size_bytes()
    max = undo_threshold_bytes()

    %{
      db_size_bytes: bytes,
      db_size_mb: div(bytes, 1_048_576),
      threshold_mb: undo_max_mb(),
      undo_available: bytes <= max,
      neon_hosted: neon_hosted?()
    }
  end

  @doc """
  Heuristic Neon-hosted detection — informational only, never an API call.

  Matches a `*.neon.tech` host (config `hostname`/`url` or `DATABASE_URL`), or
  `LAZYPOCK_NEON_HOSTED=1`. Set `LAZYPOCK_NEON_NOTICE=0` to silence the notice.
  """
  @spec neon_hosted?() :: boolean()
  def neon_hosted? do
    cond do
      System.get_env("LAZYPOCK_NEON_NOTICE") == "0" -> false
      System.get_env("LAZYPOCK_NEON_HOSTED") == "1" -> true
      true -> neon_host?(db_host())
    end
  end

  defp neon_host?(nil), do: false
  defp neon_host?(host) when is_binary(host), do: String.ends_with?(host, ".neon.tech")

  defp db_host do
    cfg = Application.get_env(:lazypock, Lazypock.Repo) || []

    cond do
      is_binary(cfg[:hostname]) ->
        cfg[:hostname]

      is_binary(cfg[:url]) ->
        cfg[:url] |> URI.parse() |> Map.get(:host)

      is_binary(System.get_env("DATABASE_URL")) ->
        System.get_env("DATABASE_URL") |> URI.parse() |> Map.get(:host)

      true ->
        nil
    end
  end

  # Names of the relations that actually exist (base tables AND views —
  # `pg_tables` would omit view collections), or nil when the lookup fails (in
  # which case callers keep the previous, unfiltered behavior).
  defp existing_tables do
    case Ecto.Adapters.SQL.query(
           Repo,
           "SELECT table_name FROM information_schema.tables WHERE table_schema = ANY (current_schemas(false))",
           []
         ) do
      {:ok, %{rows: rows}} -> rows |> Enum.map(&hd/1) |> MapSet.new()
      _ -> nil
    end
  end

  # Collections straight from the DB (no dependency on the in-process ETS
  # registry, so the CLI `lazypock backup` / `lazypock restore` commands work
  # before the full app is booted).
  defp all_collections do
    Repo.all(Lazypock.Collections.Collection)
    |> Repo.preload(:fields)
  end

  # Collection names from the DB — same reason as all_collections/0.
  defp collection_names do
    all_collections() |> Enum.map(& &1.name)
  end

  @doc """
  Imports / restores collections from a backup payload.

  Accepts the export envelope (`%{"collections" => [...]}`) or a bare list.
  Existing collections are updated (fields/rules/options/hooks), missing
  ones created, and records upserted by id via `GenericRecord.restore/2`.

  Each collection's field list may be keyed `"schema"` (LazyPock's own
  export) or `"fields"` (PocketBase 23+ export) — see
  `normalize_import_payload/1`. A collection whose field data is missing,
  ambiguous (both keys present), or malformed is reported in `errors`
  rather than silently imported with no fields. Fields marked
  `"system": true` (e.g. PocketBase's own `id` field) are dropped before
  create/update, since LazyPock's DDL layer manages its own system columns.

  `delete_missing` additionally drops user collections (and fields) absent
  from the payload — system collections are always protected.

  ## Options

    * `:atomic` (default `true`) — run the whole import in one transaction and
      roll everything back if any collection or record fails, so a bad batch
      never leaves a half-applied schema behind. When it rolls back, the
      result carries `rolled_back: true` and `imported: []` with the collected
      `errors`. Set to `false` for the old best-effort behavior (import what
      can be imported, report the rest).
    * `:snapshot` (default `true`) — capture the current database (user
      collections) before mutating so the import can be undone with
      `rollback/0`.

  Returns `%{imported: [...], errors: [...], rolled_back: boolean()}`.
  """
  @spec restore(map() | list(), boolean(), keyword()) :: map()
  def restore(payload, delete_missing \\ false, opts \\ [])

  def restore(%{"collections" => collections}, delete_missing, opts) when is_list(collections) do
    restore(collections, delete_missing, opts)
  end

  # Atom-keyed envelope (e.g. Backup.export() piped straight into restore).
  def restore(%{collections: collections}, delete_missing, opts) when is_list(collections) do
    restore(collections, delete_missing, opts)
  end

  def restore(collections, delete_missing, opts) when is_list(collections) do
    run_import(
      atomic_mode(Keyword.get(opts, :atomic, :batch)),
      collections,
      delete_missing == true,
      Keyword.get(opts, :snapshot, true),
      Keyword.get(opts, :prune, false),
      :inline
    )
  end

  @doc """
  Normalizes the `:atomic` option.

    * `true` / `:batch` (default) — one transaction around the whole import:
      any failure rolls back everything (the pre-existing contract).
    * `:per_collection` — each collection commits in its own transaction, so a
      failure only rolls that collection back and the rest continue.
    * `false` — legacy best effort: no wrapping transaction at all.
  """
  @spec atomic_mode(term()) :: :batch | :per_collection | false
  def atomic_mode(true), do: :batch
  def atomic_mode(:batch), do: :batch
  def atomic_mode(:per_collection), do: :per_collection
  def atomic_mode(false), do: false

  def atomic_mode(other) do
    Logger.warning("Backup.restore: unknown :atomic option #{inspect(other)} — using :batch")
    :batch
  end

  @doc """
  Imports an NDJSON archive produced by `export_stream/1`.

  Same `:atomic` / `:snapshot` options as `restore/3`, plus `:prune` — delete
  records that exist now but are absent from the archive, which is what makes
  `rollback/0` a true point-in-time rollback.
  """
  @spec restore_archive(String.t(), boolean(), keyword()) :: map()
  def restore_archive(path, delete_missing \\ false, opts \\ []) do
    case unpack_archive(path) do
      {:ok, scratch} ->
        try do
          {collections, root} = read_archive!(scratch)

          run_import(
            atomic_mode(Keyword.get(opts, :atomic, :batch)),
            collections,
            delete_missing == true,
            Keyword.get(opts, :snapshot, true),
            Keyword.get(opts, :prune, false),
            {:ndjson, root}
          )
        rescue
          e ->
            %{imported: [], errors: [%{name: nil, error: describe_error(e)}], rolled_back: true}
        after
          File.rm_rf(scratch)
        end

      {:error, reason} ->
        %{imported: [], errors: [%{name: nil, error: describe_error(reason)}], rolled_back: true}
    end
  end

  # Single entry point for every import shape (inline JSON records or a streamed
  # NDJSON archive) and every transaction granularity.
  defp run_import(mode, collections, delete_missing, snapshot?, prune?, records_source) do
    case mode do
      :batch ->
        restore_atomically(collections, delete_missing, snapshot?, prune?, records_source)

      :per_collection ->
        restore_per_collection(collections, delete_missing, snapshot?, prune?, records_source)

      false ->
        restore_partially(collections, delete_missing, snapshot?, prune?, records_source)
    end
  end

  # All-or-nothing: one transaction around the whole import. A failure in any
  # collection (or record) rolls back every change made by the batch.
  defp restore_atomically(collections, delete_missing, snapshot?, prune?, records_source) do
    case take_checkpoint(snapshot?) do
      {:ok, checkpoint} ->
        outcome =
          try do
            case Repo.transaction(fn ->
                   # `halt_on_error: true` stops at the first failing collection:
                   # continuing would run statements on an already-aborted
                   # transaction and report a confusing 25P02 instead of the
                   # real error.
                   result = apply_all(collections, delete_missing, true, prune?, records_source)

                   if result.errors == [], do: result, else: Repo.rollback(result)
                 end) do
              {:ok, result} ->
                Map.put(result, :rolled_back, false)

              {:error, %{errors: _} = result} ->
                resync_registry()
                result |> Map.put(:imported, []) |> Map.put(:rolled_back, true)

              {:error, reason} ->
                resync_registry()

                %{
                  imported: [],
                  errors: [%{name: nil, error: describe_error(reason)}],
                  rolled_back: true
                }
            end
          rescue
            e ->
              # A raised DB error (e.g. a bang DDL query) already rolled the
              # transaction back.
              resync_registry()

              %{imported: [], errors: [%{name: nil, error: describe_error(e)}], rolled_back: true}
          end

        finalize_checkpoint(outcome, checkpoint)

      {:error, reason} ->
        checkpoint_failed(reason)
    end
  end

  # A rolled-back batch never emits a compensating DDL broadcast, so the
  # in-memory registry may still cache collections/fields the rollback undid.
  # Resync it from the database (no-op when the app isn't booted, e.g. the
  # `lazypock restore` CLI path).
  defp resync_registry do
    case Process.whereis(Lazypock.Collections.Registry) do
      nil -> :ok
      _pid -> GenServer.call(Lazypock.Collections.Registry, :reload, 30_000)
    end
  end

  # Each collection (schema + records) commits in its own transaction, so one
  # bad collection no longer discards the ones before it.
  defp restore_per_collection(collections, delete_missing, snapshot?, prune?, records_source) do
    case take_checkpoint(snapshot?) do
      {:ok, checkpoint} ->
        existing_names = collection_names() |> MapSet.new()
        normalized = normalize_collections(collections)

        {imported, errors} =
          Enum.reduce(normalized, {[], []}, fn coll, {imp, errs} ->
            case Repo.transaction(fn ->
                   case import_one(coll, existing_names, delete_missing, records_source) do
                     {:ok, entry} -> entry
                     {:error, entry} -> Repo.rollback(entry)
                   end
                 end) do
              {:ok, entry} -> {[entry | imp], errs}
              {:error, entry} -> {imp, [entry | errs]}
            end
          end)

        if delete_missing, do: drop_missing_each(normalized, existing_names)
        if prune?, do: prune_records_not_in(keep_streams(normalized, records_source))

        finalize_checkpoint(
          %{imported: Enum.reverse(imported), errors: Enum.reverse(errors), rolled_back: false},
          checkpoint
        )

      {:error, reason} ->
        checkpoint_failed(reason)
    end
  end

  # Legacy best effort: no wrapping transaction — every DDL statement and every
  # record batch commits on its own.
  defp restore_partially(collections, delete_missing, snapshot?, prune?, records_source) do
    case take_checkpoint(snapshot?) do
      {:ok, checkpoint} ->
        outcome =
          collections
          |> apply_all(delete_missing, false, prune?, records_source)
          |> Map.put(:rolled_back, false)

        finalize_checkpoint(outcome, checkpoint)

      {:error, reason} ->
        checkpoint_failed(reason)
    end
  end

  # Normalizes any accepted payload shape, then applies every collection.
  defp apply_all(collections, delete_missing, halt_on_error, prune?, records_source) do
    collections = normalize_collections(collections)
    existing_names = collection_names() |> MapSet.new()
    incoming_names = collections |> Enum.map(& &1["name"]) |> MapSet.new()

    {imported_list, errors_list} =
      Enum.reduce_while(collections, {[], []}, fn coll_data, {imported_acc, errors_acc} ->
        case import_one(coll_data, existing_names, delete_missing, records_source) do
          {:ok, entry} ->
            {:cont, {[entry | imported_acc], errors_acc}}

          {:error, entry} ->
            next = {imported_acc, [entry | errors_acc]}
            if halt_on_error, do: {:halt, next}, else: {:cont, next}
        end
      end)

    if delete_missing do
      for name <- MapSet.difference(existing_names, incoming_names), do: drop_quietly(name)
    end

    if prune?, do: prune_records_not_in(keep_streams(collections, records_source))

    %{imported: Enum.reverse(imported_list), errors: Enum.reverse(errors_list)}
  end

  defp normalize_collections(collections) do
    collections
    |> Enum.map(&stringify_keys/1)
    |> normalize_import_payload()
    # View collections must be (re)created after the base/auth collections their
    # queries reference, so their queries can be introspected.
    |> Enum.sort_by(fn c -> if c["type"] == "view", do: 1, else: 0 end)
  end

  defp drop_quietly(name) do
    case Lazypock.Schema.DDL.drop_collection(name) do
      :ok -> :ok
      {:error, _} -> :ok
    end
  end

  # delete_missing under :per_collection — one transaction per dropped table.
  defp drop_missing_each(collections, existing_names) do
    incoming = collections |> Enum.map(& &1["name"]) |> MapSet.new()

    for name <- MapSet.difference(existing_names, incoming) do
      Repo.transaction(fn -> drop_quietly(name) end)
    end

    :ok
  end

  # Applies one collection: schema first, then its records.
  defp import_one(coll_data, existing_names, delete_missing, records_source) do
    name = coll_data["name"]

    case coll_data["__schema_error__"] do
      nil ->
        try do
          apply_collection!(coll_data, existing_names, delete_missing, records_source)
        rescue
          e -> {:error, %{name: name, error: describe_error(e)}}
        end

      reason ->
        {:error, %{name: name, error: describe_error(reason)}}
    end
  end

  defp apply_collection!(coll_data, existing_names, delete_missing, records_source) do
    name = coll_data["name"]
    type = coll_data["type"] || "base"

    case apply_schema!(coll_data, type, existing_names, delete_missing) do
      {:ok, _} ->
        case apply_records(name, type, records_for(name, type, coll_data, records_source)) do
          {:ok, count} -> {:ok, %{name: name, type: type, records_imported: count}}
          {:error, reason} -> {:error, %{name: name, error: describe_error(reason)}}
        end

      {:error, reason} ->
        {:error, %{name: name, error: describe_error(reason)}}
    end
  end

  # The DDL half of an import, split out so the archive path can create the
  # table before streaming rows into it.
  defp apply_schema!(coll_data, type, existing_names, delete_missing) do
    name = coll_data["name"]
    # View fields are derived server-side from options["view_query"] — the
    # exported schema is ignored to avoid replaying stale field metadata.
    schema = if type == "view", do: [], else: coll_data["schema"] || []
    rules = coll_data["rules"]
    options = coll_data["options"]
    hooks = coll_data["hooks"]

    # Custom indexes live inside options["indexes"] — extract so the DDL engine
    # can (re)create the actual Postgres indexes, not just the metadata. Omitted
    # when the payload doesn't carry options so existing target indexes are left
    # untouched.
    indexes =
      case options do
        %{"indexes" => idx} when is_list(idx) -> idx
        _ -> nil
      end

    if name in existing_names do
      case Lazypock.Schema.DDL.update_collection(
             name,
             [fields: schema]
             |> maybe_put(:rules, rules)
             |> maybe_put(:options, options)
             |> maybe_put(:hooks, hooks)
             |> maybe_put(:indexes, indexes)
             |> Keyword.put(:delete_missing_fields, delete_missing)
           ) do
        {:ok, _} -> {:ok, :updated}
        other -> other
      end
    else
      Lazypock.Schema.DDL.create_collection(
        name,
        [type: type, fields: schema]
        |> maybe_put(:rules, rules)
        |> maybe_put(:options, options)
        |> maybe_put(:hooks, hooks)
        |> maybe_put(:indexes, indexes)
      )
    end
  end

  # View collections are read-only (rows come from the view query), so exported
  # row data is never re-inserted.
  defp records_for(_name, "view", _coll, _source), do: []
  defp records_for(_name, _type, coll, :inline), do: coll["records"] || []
  defp records_for(name, _type, _coll, {:ndjson, root}), do: ndjson_record_stream(root, name)

  # Batched upserts via GenericRecord.restore_many/3: one statement per batch,
  # capped by the Postgres parameter limit, with oversized rows inserted alone.
  defp apply_records(_name, "view", _records), do: {:ok, 0}

  defp apply_records(name, _type, records) do
    GenericRecord.restore_many(name, records,
      batch_size: import_batch_size(),
      big_row_bytes: import_big_row_bytes()
    )
  end

  defp import_batch_size do
    case Integer.parse(System.get_env("LAZYPOCK_IMPORT_BATCH_SIZE") || "") do
      {n, _} when n > 0 -> n
      _ -> 500
    end
  end

  defp import_big_row_bytes do
    case Integer.parse(System.get_env("LAZYPOCK_IMPORT_BIG_ROW_MB") || "") do
      {n, _} when n > 0 -> n * 1_048_576
      _ -> 5_000_000
    end
  end

  # ── Archive reading ──────────────────────────────────────────────────────

  defp unpack_archive(path) do
    scratch = scratch_dir("import")
    File.mkdir_p!(scratch)

    case :zip.unzip(String.to_charlist(path), [{:cwd, String.to_charlist(scratch)}]) do
      {:ok, _files} ->
        {:ok, scratch}

      {:error, reason} ->
        File.rm_rf(scratch)
        {:error, "could not read archive #{path}: #{inspect(reason)}"}
    end
  end

  defp read_archive!(scratch) do
    manifest = scratch |> Path.join("manifest.json") |> read_json!()

    case manifest["format"] do
      @archive_format -> :ok
      other -> raise "not a LazyPock archive (format=#{inspect(other)})"
    end

    if (manifest["format_version"] || 0) > @archive_format_version do
      raise "archive format_version #{manifest["format_version"]} is newer than this " <>
              "LazyPock supports (#{@archive_format_version})"
    end

    schema = scratch |> Path.join("schema.json") |> read_json!()
    {schema["collections"] || [], scratch}
  end

  defp read_json!(path) do
    case File.read(path) do
      {:ok, body} -> Jason.decode!(body)
      {:error, reason} -> raise "cannot read #{Path.basename(path)}: #{inspect(reason)}"
    end
  end

  # One JSON object per line, decoded and handed on one at a time — a single
  # >100 MB record costs one line's worth of memory, not the whole file's.
  defp ndjson_record_stream(root, name) do
    case ndjson_path(root, name) do
      path ->
        if File.exists?(path) do
          path
          |> File.stream!(:line, [])
          |> Stream.map(&String.trim_trailing(&1, "\n"))
          |> Stream.reject(&(&1 == ""))
          |> Stream.map(&Jason.decode!/1)
        else
          []
        end
    end
  end

  defp ndjson_id_stream(root, name) do
    path = ndjson_path(root, name)

    if File.exists?(path) do
      path
      |> File.stream!(:line, [])
      |> Stream.map(&String.trim_trailing(&1, "\n"))
      |> Stream.reject(&(&1 == ""))
      |> Stream.map(fn line -> line |> Jason.decode!() |> Map.get("id") end)
      |> Stream.reject(&is_nil/1)
    else
      []
    end
  end

  # ── Undo checkpoints ─────────────────────────────────────────────────────

  # Below the size threshold an automatic pre-import checkpoint is written as an
  # NDJSON archive and only a pointer is stored — never a second copy of the DB
  # as a JSONB column. Above the threshold `:skipped` is returned; the caller is
  # expected to have taken an explicit confirmation first (see preflight/0).
  defp take_checkpoint(false), do: {:ok, :none}

  defp take_checkpoint(true) do
    if undo_available?() do
      case export_stream(kind: "checkpoint", exclude_system: true) do
        {:ok, %{path: path, bytes: bytes}} -> {:ok, {:pointer, %{path: path, bytes: bytes}}}
        {:error, reason} -> {:error, reason}
      end
    else
      {:ok, :skipped}
    end
  end

  defp finalize_checkpoint(outcome, {:pointer, pointer}) do
    if outcome.errors == [] do
      store_checkpoint(pointer)
    else
      File.rm(pointer.path)
    end

    outcome
  end

  defp finalize_checkpoint(outcome, _), do: outcome

  defp checkpoint_failed(reason) do
    %{
      imported: [],
      errors: [
        %{name: nil, error: "could not create the undo checkpoint: #{describe_error(reason)}"}
      ],
      rolled_back: true
    }
  end

  defp store_checkpoint(%{path: path, bytes: bytes}) do
    ensure_snapshot_table!()

    Ecto.Adapters.SQL.query!(
      Repo,
      "INSERT INTO #{@snapshot_table} (storage_path, byte_size, format) VALUES ($1, $2, $3)",
      [path, bytes, @archive_format]
    )

    prune_checkpoints!()
    :ok
  end

  # Keeps the newest N pointers AND removes the archives they referenced, so
  # checkpoint retention does not leak disk.
  defp prune_checkpoints! do
    keep = undo_keep()

    selector =
      "FROM #{@snapshot_table} WHERE id NOT IN (" <>
        "SELECT id FROM #{@snapshot_table} ORDER BY created_at DESC, id DESC LIMIT #{keep})"

    case Ecto.Adapters.SQL.query(Repo, "SELECT id, storage_path #{selector}", []) do
      {:ok, %{rows: rows}} ->
        Ecto.Adapters.SQL.query!(Repo, "DELETE #{selector}", [])
        for [_id, path] <- rows, is_binary(path), do: File.rm(path)
        :ok

      _ ->
        :ok
    end
  end

  # ── Pruning (true point-in-time rollback) ────────────────────────────────

  # Deletes records that exist now but were not in the checkpoint. The keep-set
  # is loaded into a TEMP TABLE rather than a MapSet, so pruning a 10 GB DB does
  # not have to hold every id (let alone every row) in memory.
  defp prune_records_not_in(nil), do: :ok

  defp prune_records_not_in(keep_streams) when is_map(keep_streams) do
    Repo.transaction(fn ->
      tables = existing_tables()

      for {name, ids} <- keep_streams,
          is_binary(name),
          not Lazypock.Collections.Collection.system?(name),
          is_nil(tables) or MapSet.member?(tables, name) do
        tmp = "_lzp_keep_" <> Integer.to_string(:erlang.phash2(name), 36)

        Ecto.Adapters.SQL.query!(
          Repo,
          "CREATE TEMP TABLE #{tmp} (id uuid PRIMARY KEY) ON COMMIT DROP",
          []
        )

        load_keep_ids!(tmp, ids)

        Ecto.Adapters.SQL.query!(
          Repo,
          "DELETE FROM #{Lazypock.Schema.TypeMapper.quote_ident(name)} t " <>
            "WHERE NOT EXISTS (SELECT 1 FROM #{tmp} k WHERE k.id = t.id)",
          []
        )

        Ecto.Adapters.SQL.query!(Repo, "DROP TABLE #{tmp}", [])
      end

      :ok
    end)

    :ok
  end

  defp load_keep_ids!(_tmp, []), do: :ok

  defp load_keep_ids!(tmp, ids) do
    ids
    |> Stream.chunk_every(2_000)
    |> Enum.each(fn chunk ->
      # `::text[]` then a SQL-side cast: the ids arrive as UUID strings (from
      # JSON), which Postgrex would reject as a `uuid[]` parameter.
      Ecto.Adapters.SQL.query!(
        Repo,
        "INSERT INTO #{tmp} (id) SELECT unnest($1::text[])::uuid ON CONFLICT DO NOTHING",
        [chunk]
      )
    end)

    :ok
  end

  # Keep-sets per collection, from either inline records or the archive's NDJSON
  # files (streamed — only ids are ever held).
  defp keep_streams(collections, :inline) do
    collections
    |> Enum.reject(&(&1["type"] == "view"))
    |> Map.new(fn c -> {c["name"], Enum.map(c["records"] || [], & &1["id"])} end)
  end

  defp keep_streams(collections, {:ndjson, root}) do
    collections
    |> Enum.reject(&(&1["type"] == "view"))
    |> Map.new(fn c -> {c["name"], ndjson_id_stream(root, c["name"])} end)
  end

  defp describe_error(%{error: error}), do: describe_error(error)
  defp describe_error(reason) when is_binary(reason), do: reason
  defp describe_error(reason) when is_exception(reason), do: Exception.message(reason)
  defp describe_error(reason), do: inspect(reason)

  # ── Import snapshots / undo checkpoints ─────────────────────────────────

  @doc """
  Metadata for the most recent undo checkpoint, or `nil` when there is none.
  Used by the Studio to offer "Undo last import".
  """
  @spec last_snapshot() :: map() | nil
  def last_snapshot do
    ensure_snapshot_table!()

    case Ecto.Adapters.SQL.query(
           Repo,
           "SELECT id::text, created_at, byte_size, format, storage_path IS NOT NULL " <>
             "FROM #{@snapshot_table} ORDER BY created_at DESC, id DESC LIMIT 1",
           []
         ) do
      {:ok, %{rows: [[id, created_at, byte_size, format, file_based]]}} ->
        %{
          id: id,
          created_at: created_at,
          byte_size: byte_size,
          format: format,
          file_based: file_based
        }

      _ ->
        nil
    end
  end

  @doc """
  Rolls the database back to the state captured before the last import.

  Restores the checkpoint (schemas, rules, indexes and records) and deletes
  records created since it was taken, so the result is a true point-in-time
  rollback. Only user collections are touched — system collections are never
  checkpointed or pruned. The checkpoint is consumed on success, so a second
  rollback is a no-op.
  """
  @spec rollback() :: {:ok, map()} | {:error, term()}
  def rollback do
    do_rollback()
  rescue
    # A raise here (malformed legacy snapshot, DDL failure, ...) becomes a
    # distinguishable error instead of an opaque 500 from the caller's view.
    e -> {:error, {:failed, Exception.message(e)}}
  end

  defp do_rollback do
    ensure_snapshot_table!()

    case Ecto.Adapters.SQL.query(
           Repo,
           "SELECT id::text, payload, storage_path FROM #{@snapshot_table} " <>
             "ORDER BY created_at DESC, id DESC LIMIT 1",
           []
         ) do
      {:ok, %{rows: [[id, payload, storage_path]]}} ->
        result =
          cond do
            is_binary(storage_path) and File.exists?(storage_path) ->
              restore_archive(storage_path, true, atomic: :batch, snapshot: false, prune: true)

            is_binary(storage_path) ->
              %{
                imported: [],
                errors: [
                  %{name: nil, error: "undo checkpoint archive is missing: #{storage_path}"}
                ],
                rolled_back: true
              }

            true ->
              # Legacy JSONB snapshot written before checkpoints became files.
              restore(decode_payload(payload), true,
                atomic: :batch,
                snapshot: false,
                prune: true
              )
          end

        if result.errors == [] do
          # `id` is selected as ::text above. The inner `::text` is required: a
          # bare `$1::uuid` would make Postgres report the *parameter* as uuid, and
          # Postgrex would then demand 16 raw bytes instead of the string.
          Ecto.Adapters.SQL.query!(
            Repo,
            "DELETE FROM #{@snapshot_table} WHERE id = $1::text::uuid",
            [id]
          )

          if is_binary(storage_path), do: File.rm(storage_path)
          {:ok, result}
        else
          {:error, result}
        end

      _ ->
        {:error, :no_snapshot}
    end
  end

  # Postgrex returns `jsonb` columns as a binary by default.
  defp decode_payload(payload) when is_binary(payload), do: Jason.decode!(payload)
  defp decode_payload(payload), do: payload

  # Self-healing so the CLI `lazypock restore` also works on an instance whose
  # migrations are older than this feature. Checkpoints are archives on disk
  # with only a pointer stored here, so `payload` must be nullable.
  defp ensure_snapshot_table! do
    Ecto.Adapters.SQL.query!(
      Repo,
      """
      CREATE TABLE IF NOT EXISTS #{@snapshot_table} (
        id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
        created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
        payload JSONB,
        storage_path TEXT,
        byte_size BIGINT,
        format TEXT
      )
      """,
      []
    )

    # Upgrade a table created by an older LazyPock (payload NOT NULL and no
    # pointer columns).
    Ecto.Adapters.SQL.query!(
      Repo,
      "ALTER TABLE #{@snapshot_table} ALTER COLUMN payload DROP NOT NULL",
      []
    )

    Ecto.Adapters.SQL.query!(
      Repo,
      "ALTER TABLE #{@snapshot_table} ADD COLUMN IF NOT EXISTS storage_path TEXT",
      []
    )

    Ecto.Adapters.SQL.query!(
      Repo,
      "ALTER TABLE #{@snapshot_table} ADD COLUMN IF NOT EXISTS byte_size BIGINT",
      []
    )

    Ecto.Adapters.SQL.query!(
      Repo,
      "ALTER TABLE #{@snapshot_table} ADD COLUMN IF NOT EXISTS format TEXT",
      []
    )

    :ok
  end

  # ── Field-list key resolution (schema vs. fields) ─────────────────────────

  # Resolves the raw field list for one collection map, honoring both key
  # names LazyPock accepts: "schema" (its own export) and "fields"
  # (PocketBase 23+ export). Returns {:ok, list} | {:error, reason} — never
  # implicitly defaults to [] except for view collections (fields are
  # derived server-side) and a list that is explicitly empty in the payload.
  defp resolve_schema(%{"type" => "view"}), do: {:ok, []}

  defp resolve_schema(c) do
    case {Map.fetch(c, "fields"), Map.fetch(c, "schema")} do
      {{:ok, fields}, :error} when is_list(fields) ->
        {:ok, fields}

      {:error, {:ok, schema}} when is_list(schema) ->
        {:ok, schema}

      {{:ok, _}, {:ok, _}} ->
        {:error,
         "collection #{inspect(c["name"])}: payload has both \"fields\" and " <>
           "\"schema\" — ambiguous, refusing to guess"}

      {{:ok, other}, _} when not is_list(other) ->
        {:error, "collection #{inspect(c["name"])}: \"fields\" must be a list"}

      {_, {:ok, other}} when not is_list(other) ->
        {:error, "collection #{inspect(c["name"])}: \"schema\" must be a list"}

      {:error, :error} ->
        {:error,
         "collection #{inspect(c["name"])}: missing both \"fields\" and " <>
           "\"schema\" — expected at least an empty list"}
    end
  end

  # Validates a single field entry is at least structurally a map. Both flat
  # fields (settings directly on the field, PocketBase 23+ shape) and fields
  # with settings nested under "options" (relation's collectionId, but also
  # ordinary min/max/values-style settings on other types) are accepted —
  # the DDL/field-metadata layer already handles both shapes on its own
  # (confirmed by the existing PocketBase-import test suite, which imports
  # text/number/select fields carrying non-empty "options" successfully).
  # An earlier version of this function rejected non-relation fields with
  # nested "options" as an assumed-unsupported "PocketBase <23" shape — that
  # assumption was never verified against the actual DDL code and directly
  # contradicted this existing, passing test coverage, so it's removed.
  defp validate_field(f) when is_map(f), do: {:ok, f}

  defp validate_field(other) do
    {:error, "field entry must be a map, got #{inspect(other)}"}
  end

  # Drops fields the payload marks as system-managed (e.g. PocketBase's own
  # "id" primary key field, "system": true). LazyPock's DDL layer creates
  # its own system columns when a collection is created — re-declaring one
  # explicitly (as PocketBase's export does for "id") collides with it
  # (Postgrex 42701 duplicate_column) rather than being a legitimate custom
  # field, so these are removed rather than validated/imported.
  defp strip_system_fields(fields) do
    Enum.reject(fields, fn f -> f["system"] == true end)
  end

  # resolve_schema/1 + validate_field/1 for one collection, collapsed to a
  # single {:ok, list} | {:error, reason} so normalize_import_payload/1
  # doesn't need to thread multiple failure shapes through its Enum.map.
  # System fields are stripped last, after validation, so a malformed field
  # (e.g. not a map at all) still surfaces as an error rather than being
  # masked by unrelated system fields elsewhere in the same list.
  defp resolve_and_validate_schema(c) do
    with {:ok, raw_fields} <- resolve_schema(c) do
      raw_fields
      |> Enum.reduce_while({:ok, []}, fn field, {:ok, acc} ->
        case validate_field(field) do
          {:ok, valid} -> {:cont, {:ok, [valid | acc]}}
          {:error, reason} -> {:halt, {:error, reason}}
        end
      end)
      |> case do
        {:ok, list} -> {:ok, list |> Enum.reverse() |> strip_system_fields()}
        {:error, _} = err -> err
      end
    end
  end

  # ── PocketBase-format payload normalization ──────────────────────────────

  # PocketBase exports use camelCase field names, PB collection ids in relation
  # fields. Field names are kept VERBATIM (no snake_case/camelCase conversion)
  # — whatever the payload says (tagColor or tag_color) becomes the field name
  # and the API/codegen name; the DB column is derived verbatim from the field name by the
  # DDL/FieldNames layers (case preserved). The only exception: an incoming name that matches
  # an EXISTING column by its snake_case form (e.g. system users'
  # `passwordHash` ↔ LazyPock's `password_hash`) is aliased to that column so
  # no duplicate is created.
  #
  # Each collection's field list is resolved via resolve_and_validate_schema/1
  # first (honoring both the "schema" and "fields" keys, rejecting
  # malformed/ambiguous field data, and stripping system-managed fields) —
  # a collection that fails this step is tagged
  # with "__schema_error__" instead of being processed, so restore/2 can
  # report it in `errors` without ever calling
  # create_collection/update_collection with an incorrectly-emptied (or
  # duplicate-"id"-colliding) field list.
  defp normalize_import_payload(collections) do
    id_to_name = Map.new(collections, fn c -> {c["id"], c["name"]} end)

    Enum.map(collections, fn c ->
      case resolve_and_validate_schema(c) do
        {:ok, raw_fields} ->
          {fields, name_map} = reconcile_field_names(c["name"], raw_fields, id_to_name)

          c
          |> Map.put("schema", fields)
          |> Map.put("records", rekey_records(c["records"] || [], name_map))

        {:error, reason} ->
          Map.put(c, "__schema_error__", reason)
      end
    end)
  end

  # ── Field options normalization ─────────────────────────

  # PocketBase exports put type-specific settings at the TOP LEVEL of each
  # field map ("maxSelect", "values", "onCreate"...) while LazyPock's DDL and
  # field-metadata layers read them from the field's nested "options" map.
  # Without this step, a multi-select field with "maxSelect": > 1 would be
  # created as plain TEXT instead of JSONB (→ "Postgrex expected a binary,
  # got [...]"), and autodate fields would never stamp their column.
  #
  # Idempotent: fields that already nest their settings under "options" pass
  # through unchanged (Map.put_new won't overwrite; Map.delete is a no-op).
  @field_top_level_option_keys %{
    "text" => ["min", "max", "pattern", "autogeneratePattern"],
    "editor" => ["convertURLs"],
    "number" => ["min", "max", "noDecimal", "onlyInt"],
    "date" => ["min", "max"],
    "autodate" => ["onCreate", "onUpdate"],
    "select" => ["values", "maxSelect"],
    "file" => ["maxSelect", "maxSize", "mimeTypes", "thumbs", "protected"],
    "relation" => ["collectionId", "cascadeDelete", "minSelect", "maxSelect", "displayFields"],
    "url" => ["exceptDomains", "onlyDomains"],
    "email" => ["exceptDomains", "onlyDomains"],
    "password" => ["min", "max", "pattern"],
    "json" => ["maxSize"]
  }

  defp normalize_field_options(field) when is_map(field) do
    case Map.get(@field_top_level_option_keys, field["type"], []) do
      [] ->
        field

      keys ->
        opts = Map.get(field, "options") || %{}

        new_opts =
          Enum.reduce(keys, opts, fn key, acc ->
            case Map.fetch(field, key) do
              {:ok, v} -> Map.put_new(acc, key, v)
              :error -> acc
            end
          end)

        cleaned = Enum.reduce(keys, field, fn key, acc -> Map.delete(acc, key) end)

        Map.put(cleaned, "options", new_opts)
    end
  end

  # Returns {fields, %{payload_name => field_name}}. New fields keep their
  # payload name verbatim; existing fields are matched by exact name first,
  # then by snake_case-normalized name (passwordHash → password_hash).
  defp reconcile_field_names(collection_name, fields, id_to_name) do
    existing_names = existing_field_names(collection_name)

    {fields, name_map} =
      Enum.reduce(fields, {[], %{}}, fn f, {acc, name_map} ->
        payload_name = f["name"]
        lazy_name = match_existing_name(payload_name, existing_names)

        normalized =
          f
          |> Map.put("name", lazy_name)
          |> normalize_field_options()
          |> resolve_relation(id_to_name)

        {[normalized | acc], Map.put(name_map, payload_name, lazy_name)}
      end)

    {Enum.reverse(fields), name_map}
  end

  defp existing_field_names(collection_name) do
    case Repo.one(
           from(c in Lazypock.Collections.Collection,
             where: c.name == ^collection_name,
             preload: [:fields]
           )
         ) do
      nil -> MapSet.new()
      coll -> MapSet.new(coll.fields, & &1.name)
    end
  end

  # Exact match wins; else an existing field whose snake_case form equals the
  # payload's (PB camelCase ↔ LZ snake_case system columns); else verbatim.
  defp match_existing_name(payload_name, existing_names) when is_binary(payload_name) do
    cond do
      MapSet.member?(existing_names, payload_name) ->
        payload_name

      true ->
        normalized = normalize_field_name(payload_name)

        Enum.find(existing_names, fn n ->
          is_binary(n) and normalize_field_name(n) == normalized
        end) || payload_name
    end
  end

  defp match_existing_name(nil, _existing_names), do: nil

  # Normalize a name to its snake_case form for MATCHING ONLY (never to rename
  # fields — those stay verbatim).
  defp normalize_field_name(name) when is_binary(name) do
    name
    |> Macro.underscore()
    |> String.replace(~r/[^a-z0-9_]/, "_")
    |> String.trim("_")
  end

  # PocketBase's users auth collection ids (older `_pb_users_auth_` and the
  # bare `pb_users_auth`) map to LazyPock's built-in `users` collection.
  @pocketbase_users_ids ["_pb_users_auth_", "pb_users_auth"]

  # Relations in PocketBase exports reference the target by PB collection id
  # (e.g. "kanban_columns_col", or PocketBase's users auth id). Resolve to the
  # LazyPock collection NAME and store it in options["collection"].
  defp resolve_relation(field, id_to_name) do
    if field["type"] == "relation" do
      opts = Map.get(field, "options", %{})
      raw_id = opts["collectionId"] || field["collectionId"]

      resolved =
        cond do
          is_binary(raw_id) and raw_id != "" and Map.has_key?(id_to_name, raw_id) ->
            id_to_name[raw_id]

          is_binary(raw_id) and raw_id in @pocketbase_users_ids ->
            pocketbase_users_name()

          true ->
            nil
        end

      if resolved do
        Map.put(field, "options", Map.put(opts, "collection", resolved))
      else
        if is_binary(raw_id) and raw_id in @pocketbase_users_ids do
          Logger.warning(
            "Relation field '#{field["name"]}' references PocketBase's users auth " <>
              "collection (#{raw_id}), but no 'users' collection exists on this instance " <>
              "— the relation was left unresolved."
          )
        end

        field
      end
    else
      field
    end
  end

  defp pocketbase_users_name do
    if "users" in collection_names(), do: "users"
  end

  # PB record keys are the PB field names (camelCase) — re-key to the
  # normalized LazyPock field names so inserts hit the right columns. PB-only
  # timestamp keys are dropped (restore sets its own created_at/updated_at
  # when they're absent).
  defp rekey_records(records, name_map) do
    Enum.map(records, fn record ->
      record
      |> Map.drop(["created", "updated"])
      |> Map.new(fn {k, v} ->
        key = to_string(k)
        {Map.get(name_map, key, key), v}
      end)
    end)
  end

  # Adds `{key, value}` to a keyword list only when value is not nil.
  # Used by import to avoid overwriting existing collection metadata
  # (rules/options/hooks/indexes) when the payload omits them.
  defp maybe_put(opts, _key, nil), do: opts
  defp maybe_put(opts, key, value), do: Keyword.put(opts, key, value)

  # Normalize an atom-keyed map (in-VM callers, e.g. Backup.export() piped
  # straight into restore) to string keys, so one code path handles both
  # in-VM payloads and JSON-decoded files.
  defp stringify_keys(map) when is_map(map) do
    Map.new(map, fn {k, v} -> {to_string(k), v} end)
  end

  defp stringify_keys(other), do: other
end
