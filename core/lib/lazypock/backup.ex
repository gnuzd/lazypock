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
  or a bare collection list, in both the LazyPock backup format and the
  PocketBase export format (camelCase field names, PB collection ids —
  see `normalize_import_payload/1`).

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
    collections =
      all_collections()
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

  `delete_missing` additionally drops user collections (and fields) absent
  from the payload — system collections are always protected.

  Returns `%{imported: [...], errors: [...]}`.
  """
  @spec restore(map() | list(), boolean()) :: %{imported: list(map()), errors: list(map())}
  def restore(payload, delete_missing \\ false)

  def restore(%{"collections" => collections}, delete_missing) when is_list(collections) do
    restore(collections, delete_missing)
  end

  # Atom-keyed envelope (e.g. Backup.export() piped straight into restore).
  def restore(%{collections: collections}, delete_missing) when is_list(collections) do
    restore(collections, delete_missing)
  end

  def restore(collections, delete_missing) when is_list(collections) do
    delete_missing = delete_missing == true

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
      Enum.reduce(collections, {[], []}, fn coll_data, {imported_acc, errors_acc} ->
        name = coll_data["name"]
        type = coll_data["type"] || "base"
        # View fields are derived server-side from options["view_query"] — the
        # exported schema is ignored to avoid replaying stale field metadata.
        schema = if type == "view", do: [], else: coll_data["schema"] || []
        records = coll_data["records"] || []
        rules = coll_data["rules"]
        options = coll_data["options"]
        hooks = coll_data["hooks"]

        # Custom indexes live inside options["indexes"] — extract so the DDL
        # engine can (re)create the actual Postgres indexes, not just the
        # metadata. Omitted when the payload doesn't carry options so existing
        # target indexes are left untouched.
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
            # View collections are read-only: rows are regenerated by the view
            # query, so exported row data is never re-inserted.
            inserted =
              if type == "view" do
                {:ok, 0}
              else
                Enum.reduce_while(records, {:ok, 0}, fn record, {:ok, count} ->
                  case GenericRecord.restore(name, record) do
                    {:ok, _} -> {:cont, {:ok, count + 1}}
                    {:error, _reason} -> {:cont, {:ok, count}}
                  end
                end)
              end

            insert_count =
              case inserted do
                {:ok, c} -> c
                _ -> 0
              end

            {[%{name: name, type: type, records_imported: insert_count} | imported_acc],
             errors_acc}

          {:error, reason} ->
            {imported_acc, [%{name: name, error: reason} | errors_acc]}
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

  # ── PocketBase-format payload normalization ──────────────────────────────

  # PocketBase exports use camelCase field names, PB collection ids in relation
  # fields. Field names are kept VERBATIM (no snake_case/camelCase conversion)
  # — whatever the payload says (tagColor or tag_color) becomes the field name
  # and the API/codegen name; the DB column is derived (lowercased) by the
  # DDL/FieldNames layers. The only exception: an incoming name that matches
  # an EXISTING column by its snake_case form (e.g. system users'
  # `passwordHash` ↔ LazyPock's `password_hash`) is aliased to that column so
  # no duplicate is created.
  defp normalize_import_payload(collections) do
    id_to_name = Map.new(collections, fn c -> {c["id"], c["name"]} end)

    Enum.map(collections, fn c ->
      {fields, name_map} = reconcile_field_names(c["name"], c["schema"] || [], id_to_name)

      c
      |> Map.put("schema", fields)
      |> Map.put("records", rekey_records(c["records"] || [], name_map))
    end)
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
