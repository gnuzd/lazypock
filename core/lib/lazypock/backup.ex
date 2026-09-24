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
        records = GenericRecord.all(coll.name)

        %{
          id: coll.id,
          name: coll.name,
          type: coll.type,
          schema: coll.schema,
          rules: coll.rules,
          options: coll.options,
          hooks: coll.hooks,
          records: records
        }
      end)

    %{collections: collections}
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
    delete_missing = delete_missing == true
    atomic = Keyword.get(opts, :atomic, true)
    snapshot? = Keyword.get(opts, :snapshot, true)

    if atomic do
      restore_atomically(collections, delete_missing, snapshot?)
    else
      restore_partially(collections, delete_missing, snapshot?)
    end
  end

  # All-or-nothing: one transaction around the whole import. A failure in any
  # collection (or record) rolls back every change made by the batch.
  defp restore_atomically(collections, delete_missing, snapshot?) do
    if snapshot?, do: ensure_snapshot_table!()

    try do
      case Repo.transaction(fn ->
             # Read the pre-import state inside the transaction, before any
             # mutation, so the snapshot and the import commit (or roll back)
             # together.
             pre = if snapshot?, do: user_snapshot()

             # `atomic: true` stops at the first failing collection: continuing
             # would run statements on an already-aborted transaction and report
             # a confusing 25P02 ("current transaction is aborted") instead of
             # the real error.
             result = do_restore(collections, delete_missing, true)

             if result.errors == [] do
               if pre, do: store_snapshot(pre)
               Map.put(result, :rolled_back, false)
             else
               Repo.rollback(result)
             end
           end) do
        {:ok, result} ->
          result

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

  # Best effort (the previous default): apply what can be applied and report
  # the rest. Each collection is still individually transactional.
  defp restore_partially(collections, delete_missing, snapshot?) do
    if snapshot?, do: ensure_snapshot_table!()
    pre = if snapshot?, do: user_snapshot()

    result = do_restore(collections, delete_missing, false)
    if pre, do: store_snapshot(pre)

    Map.put(result, :rolled_back, false)
  end

  defp do_restore(collections, delete_missing, atomic) do
    collections =
      collections
      |> Enum.map(&stringify_keys/1)
      |> normalize_import_payload()
      # View collections must be (re)created after the base/auth collections
      # their queries reference, so their queries can be introspected.
      |> Enum.sort_by(fn c -> if c["type"] == "view", do: 1, else: 0 end)

    existing_names = collection_names() |> MapSet.new()
    incoming_names = Enum.map(collections, & &1["name"]) |> MapSet.new()

    {imported_list, errors_list} =
      Enum.reduce_while(collections, {[], []}, fn coll_data, {imported_acc, errors_acc} ->
        case import_collection(coll_data, existing_names, delete_missing) do
          {:ok, entry} ->
            {:cont, {[entry | imported_acc], errors_acc}}

          {:error, entry} ->
            next = {imported_acc, [entry | errors_acc]}
            if atomic, do: {:halt, next}, else: {:cont, next}
        end
      end)

    # Delete missing collections if requested (system collections are
    # protected inside drop_collection).
    if delete_missing do
      for name <- MapSet.difference(existing_names, incoming_names) do
        case Lazypock.Schema.DDL.drop_collection(name) do
          :ok -> :ok
          {:error, _} -> :ok
        end
      end
    end

    %{imported: Enum.reverse(imported_list), errors: Enum.reverse(errors_list)}
  end

  # Imports one collection: returns {:ok, entry} or {:error, %{name:, error:}}.
  # A raised DB error (e.g. a bang DDL query) is converted to an error entry so
  # the caller can report it and (in atomic mode) roll back cleanly.
  defp import_collection(coll_data, existing_names, delete_missing) do
    name = coll_data["name"]

    case coll_data["__schema_error__"] do
      nil ->
        try do
          import_collection!(coll_data, existing_names, delete_missing)
        rescue
          e -> {:error, %{name: name, error: describe_error(e)}}
        end

      reason ->
        {:error, %{name: name, error: describe_error(reason)}}
    end
  end

  defp import_collection!(coll_data, existing_names, delete_missing) do
    name = coll_data["name"]
    type = coll_data["type"] || "base"
    # View fields are derived server-side from options["view_query"] — the
    # exported schema is ignored to avoid replaying stale field metadata.
    schema = if type == "view", do: [], else: coll_data["schema"] || []
    records = coll_data["records"] || []
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

    result =
      if name in existing_names do
        # Update existing collection — apply new schema fields plus any
        # rules/options/hooks carried in the payload.
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

    case result do
      {:ok, _} ->
        case restore_records(name, type, records) do
          {:ok, count} -> {:ok, %{name: name, type: type, records_imported: count}}
          {:error, reason} -> {:error, %{name: name, error: describe_error(reason)}}
        end

      {:error, reason} ->
        {:error, %{name: name, error: describe_error(reason)}}
    end
  end

  # View collections are read-only (rows come from the view query), so exported
  # row data is never re-inserted. Record errors are surfaced (previously they
  # were swallowed as `records_imported: 0`).
  defp restore_records(_name, "view", _records), do: {:ok, 0}

  defp restore_records(name, _type, records) do
    Enum.reduce_while(records, {:ok, 0}, fn record, {:ok, count} ->
      case GenericRecord.restore(name, record) do
        {:ok, _} -> {:cont, {:ok, count + 1}}
        {:error, reason} -> {:halt, {:error, reason}}
      end
    end)
  end

  defp describe_error(%{error: error}), do: describe_error(error)
  defp describe_error(reason) when is_binary(reason), do: reason
  defp describe_error(reason) when is_exception(reason), do: Exception.message(reason)
  defp describe_error(reason), do: inspect(reason)

  # ── Import snapshots (undo) ───────────────────────────────────────────────

  @snapshot_table "_import_snapshots"
  # Keep a short history so a repeated import can still be undone.
  @kept_snapshots 5

  @doc """
  Metadata for the most recent import snapshot, or `nil` when there is none.
  Used by the Studio to offer "Undo last import".
  """
  @spec last_snapshot() :: %{id: binary(), created_at: DateTime.t()} | nil
  def last_snapshot do
    ensure_snapshot_table!()

    case Ecto.Adapters.SQL.query(
           Repo,
           "SELECT id::text, created_at FROM #{@snapshot_table} ORDER BY created_at DESC, id DESC LIMIT 1",
           []
         ) do
      {:ok, %{rows: [[id, created_at]]}} -> %{id: id, created_at: created_at}
      _ -> nil
    end
  end

  @doc """
  Rolls the database back to the state captured before the last import.

  Restores the snapshot (schemas, rules, indexes and records) and additionally
  deletes records created since it was taken, so the result is a true
  point-in-time rollback. Only user collections are touched — system
  collections are never snapshotted or pruned. The snapshot is consumed on
  success, so a second rollback is a no-op.
  """
  @spec rollback() :: {:ok, map()} | {:error, term()}
  def rollback do
    ensure_snapshot_table!()

    case Ecto.Adapters.SQL.query(
           Repo,
           "SELECT id, payload FROM #{@snapshot_table} ORDER BY created_at DESC, id DESC LIMIT 1",
           []
         ) do
      {:ok, %{rows: [[id, payload]]}} ->
        payload = decode_payload(payload)

        case Repo.transaction(fn ->
               result = restore(payload, true, atomic: true, snapshot: false)

               if result.errors == [] do
                 prune_records_not_in(payload)

                 Ecto.Adapters.SQL.query!(Repo, "DELETE FROM #{@snapshot_table} WHERE id = $1", [
                   id
                 ])

                 result
               else
                 Repo.rollback(result)
               end
             end) do
          {:ok, result} -> {:ok, result}
          {:error, %{errors: _} = result} -> {:error, result}
          {:error, reason} -> {:error, reason}
        end

      _ ->
        {:error, :no_snapshot}
    end
  end

  # Postgrex returns `jsonb` columns as a binary by default.
  defp decode_payload(payload) when is_binary(payload), do: Jason.decode!(payload)
  defp decode_payload(payload), do: payload

  # Snapshot of the user collections only — system tables (_superusers, _otps,
  # ...) are excluded so a rollback can never rewrite or prune them.
  defp user_snapshot do
    %{collections: collections} = export()

    %{
      collections: Enum.reject(collections, &Lazypock.Collections.Collection.system?(&1.name))
    }
  end

  defp store_snapshot(payload) do
    Ecto.Adapters.SQL.query!(
      Repo,
      "INSERT INTO #{@snapshot_table} (payload) VALUES ($1)",
      [Jason.encode!(payload)]
    )

    Ecto.Adapters.SQL.query!(
      Repo,
      "DELETE FROM #{@snapshot_table} WHERE id NOT IN (" <>
        "SELECT id FROM #{@snapshot_table} ORDER BY created_at DESC, id DESC LIMIT #{@kept_snapshots})",
      []
    )

    :ok
  end

  # Self-healing so the CLI `lazypock restore` also works on an instance whose
  # migrations are older than this feature.
  defp ensure_snapshot_table! do
    Ecto.Adapters.SQL.query!(
      Repo,
      """
      CREATE TABLE IF NOT EXISTS #{@snapshot_table} (
        id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
        created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
        payload JSONB NOT NULL
      )
      """,
      []
    )

    :ok
  end

  # Deletes records that exist now but were not in the snapshot. Restore/2
  # upserts the snapshot's records but never removes extra ones, so this closes
  # the gap for a true rollback.
  defp prune_records_not_in(%{"collections" => collections}) when is_list(collections) do
    for coll <- collections, (coll["type"] || "base") != "view" do
      name = coll["name"]

      if is_binary(name) and not Lazypock.Collections.Collection.system?(name) do
        keep = MapSet.new(coll["records"] || [], & &1["id"])

        for record <- GenericRecord.all(name), not MapSet.member?(keep, record["id"]) do
          GenericRecord.delete(name, record["id"])
        end
      end
    end

    :ok
  end

  defp prune_records_not_in(_), do: :ok

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
