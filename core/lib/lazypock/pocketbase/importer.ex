defmodule Lazypock.PocketBase.Importer do
  @moduledoc """
  Imports a PocketBase instance (SQLite `pb_data/data.db` + `storage/`) into a
  LazyPock/Postgres instance.

  ## What is migrated

    * non-system collections (schema, rules, options)
    * records, preserving `created`/`updated` timestamps
    * relations (PocketBase record ids are rewritten to deterministic UUIDv5
      ids; relation field values are rewritten to match)
    * auth collections (email, bcrypt password hash, verified, emailVisibility)
    * OAuth external-auth links (`_externalAuths` → `_external_auths`)
    * files (copied from `pb_data/storage/` into LazyPock's storage; file
      field values rewritten from filenames to LazyPock file ids)

  ## ID strategy

  PocketBase record ids are 15-char base62 strings; LazyPock uses UUID primary
  keys. To keep relations intact we map each PocketBase id to a **deterministic
  UUIDv5** derived from `"<collection>#<id>"`, so re-running the import on the
  same source produces the same ids. The `--id-map` JSON report lets you look
  up old → new ids.

  ## Requirements

  The `sqlite3` CLI must be on PATH (used read-only via `-readonly -json`).
  """

  require Logger
  import Bitwise

  alias Lazypock.Schema.DDL
  alias Lazypock.Collections.Registry
  alias Lazypock.Repo
  alias Lazypock.Schema.TypeMapper

  @default_options [
    pb_dir: "pb_data",
    pb_db: nil,
    storage_dir: nil,
    dry_run: false,
    id_map_file: "pocketbase_id_map.json",
    yes: false
  ]

  @doc """
  Runs the import. Returns a summary map.
  """
  @spec import_all(keyword()) :: map()
  def import_all(opts \\ []) do
    opts = Keyword.merge(@default_options, opts)

    db_path = resolve_db_path(opts)
    storage_dir = opts[:storage_dir] || Path.join(db_path |> Path.dirname(), "storage")

    check_sqlite!()
    check_db!(db_path)

    collections = read_collections(db_path)
    user_collections = Enum.reject(collections, &system?/1)

    if user_collections == [] do
      Logger.warning("No user collections found in #{db_path} — nothing to import.")
    end

    if opts[:dry_run] do
      dry_run_summary(collections, user_collections)
    else
      do_import(db_path, storage_dir, user_collections, opts)
    end
  end

  # ── SQLite access (sqlite3 CLI, read-only) ────────────

  @doc "Runs a read-only sqlite3 query returning a list of maps ([] on error)."
  @spec query(String.t(), String.t()) :: [map()]
  def query(db_path, sql) do
    case System.cmd("sqlite3", ["-readonly", "-json", db_path, sql], stderr_to_stdout: true) do
      {out, 0} ->
        case Jason.decode(out) do
          {:ok, list} when is_list(list) -> list
          _ -> []
        end

      _ ->
        []
    end
  end

  defp check_sqlite! do
    case System.find_executable("sqlite3") do
      nil ->
        raise "sqlite3 CLI not found on PATH — required to read PocketBase's data.db"

      _ ->
        :ok
    end
  end

  defp check_db!(path) do
    unless File.exists?(path) do
      raise "PocketBase database not found at #{path}. " <>
              "Pass --pb-dir=<pb_data dir> or --pb-db=<path to data.db>"
    end

    :ok
  end

  defp resolve_db_path(opts) do
    case opts[:pb_db] do
      path when is_binary(path) and path != "" -> path
      _ -> Path.join(opts[:pb_dir], "data.db")
    end
  end

  defp system?(coll), do: coll["system"] == 1 or coll["system"] == true

  defp read_collections(db_path) do
    query(db_path, "SELECT * FROM _collections")
    |> Enum.map(fn coll ->
      coll
      |> decode_json_field("schema", [])
      |> decode_json_field("options", %{})
      |> decode_json_field("indexes", [])
    end)
  end

  defp decode_json_field(map, key, default) do
    case Map.get(map, key) do
      value when is_binary(value) ->
        case Jason.decode(value) do
          {:ok, decoded} -> Map.put(map, key, decoded)
          _ -> Map.put(map, key, default)
        end

      _ ->
        map
    end
  end

  # ── Dry run ───────────────────────────────────────────

  defp dry_run_summary(collections, user_collections) do
    %{
      dry_run: true,
      source: "PocketBase",
      collections: length(user_collections),
      skipped_system: length(collections) - length(user_collections),
      collections_list: Enum.map(user_collections, & &1["name"]),
      warnings: [
        "No changes were made (--dry-run). PocketBase record ids will be " <>
          "rewritten to deterministic UUIDv5 ids; relations preserved."
      ]
    }
  end

  # ── Import ────────────────────────────────────────────

  defp do_import(db_path, storage_dir, collections, opts) do
    # View collections must be created after the base/auth collections their
    # queries reference (PocketBase imports them in any order; LazyPock's
    # views introspect the query at creation time). Stable sort keeps the
    # original relative order within each group.
    collections =
      Enum.sort_by(collections, fn c -> if c["type"] == "view", do: 1, else: 0 end)

    # PocketBase collection-id -> collection-name map (relation fields reference
    # the target by PB collection id; LazyPock relations use collection names).
    id_to_name = Map.new(collections, &{&1["id"], &1["name"]})

    # Pre-build the COMPLETE old-id -> new-uuid map (uuid5 is deterministic),
    # so relations can be rewritten even when the target record is imported
    # after the source record (cross-collection / out-of-order references).
    full_id_map = build_id_map(db_path, collections)

    {imported, id_map, warnings} =
      Enum.reduce(collections, {%{}, %{}, []}, fn pb_coll, {imported, id_map, warnings} ->
        case import_collection(db_path, storage_dir, pb_coll, id_to_name, full_id_map, opts) do
          {:ok, result, new_warnings} ->
            {_name, coll_result} = result |> Enum.to_list() |> hd()

            {Map.merge(imported, result), Map.merge(id_map, coll_result.id_map),
             warnings ++ new_warnings}

          {:error, reason} ->
            {imported, id_map, warnings ++ ["#{pb_coll["name"]}: #{reason}"]}
        end
      end)

    if is_binary(opts[:id_map_file]) and map_size(id_map) > 0 do
      File.write!(opts[:id_map_file], Jason.encode!(%{"mapping" => id_map}, pretty: true))
      Logger.info("ID map written to #{opts[:id_map_file]}")
    end

    %{
      dry_run: false,
      collections: length(collections),
      records: Enum.sum(Map.values(imported) |> Enum.map(& &1.records)),
      files: Enum.sum(Map.values(imported) |> Enum.map(& &1.files)),
      external_auths: Enum.sum(Map.values(imported) |> Enum.map(& &1.external_auths)),
      id_map_file: opts[:id_map_file],
      warnings: warnings
    }
  end

  defp import_collection(db_path, storage_dir, pb_coll, id_to_name, full_id_map, opts) do
    name = pb_coll["name"]
    Logger.info("Importing collection: #{name} (#{pb_coll["type"]})")

    with {:ok, lazy_coll} <- import_collection_schema(pb_coll, id_to_name, opts) do
      # View collections are read-only: they have no stored records, so the
      # row data is not imported (it's regenerated by the view query).
      if lazy_coll.type == "view" do
        result = %{records: 0, files: 0, external_auths: 0, id_map: %{}}
        {:ok, %{name => result}, []}
      else
        records = read_records(db_path, name)

        {count, files, auths, id_map, rec_warnings} =
          import_records(lazy_coll, pb_coll, records, storage_dir, db_path, full_id_map)

        result = %{
          records: count,
          files: files,
          external_auths: auths,
          id_map: id_map
        }

        {:ok, %{name => result}, rec_warnings}
      end
    end
  end

  defp read_records(db_path, table) do
    query(db_path, "SELECT * FROM \"#{escape_sql_string(table)}\"")
  end

  # Creates (or adopts) the collection schema and returns the LazyPock
  # collection struct, or {:error, reason}.
  defp import_collection_schema(pb_coll, id_to_name, opts) do
    name = pb_coll["name"]

    type =
      case pb_coll["type"] do
        "auth" -> "auth"
        "view" -> "view"
        _ -> "base"
      end

    # View collections: fields are derived server-side from the view query,
    # so PB's exported schema is ignored and the query drives introspection.
    {fields, options} =
      if type == "view" do
        {[], %{"view_query" => pb_coll["viewQuery"]}}
      else
        fields =
          (pb_coll["schema"] || [])
          |> Enum.map(&map_field(&1, id_to_name))
          |> Enum.reject(&is_nil/1)

        {fields, nil}
      end

    rules = pb_rules_to_lazypock(pb_coll)
    indexes = if type == "view", do: [], else: pb_indexes_to_lazypock(pb_coll["indexes"])

    case DDL.create_collection(name,
           type: type,
           fields: fields,
           indexes: indexes,
           options: options
         ) do
      {:ok, coll} ->
        # Apply PocketBase rules (DDL sets its own defaults). The created
        # collection struct already carries the imported custom indexes.
        coll = apply_rules(coll, rules)

        # Auth collections: ensure LazyPock auth system columns exist
        # (verified, email_visibility) so email-verification and
        # email-visibility flows work even if PB's schema lacked them.
        if type == "auth" do
          ensure_auth_columns!(name)
        end

        Registry.reload!()
        {:ok, coll}

      {:error, msg} when is_binary(msg) ->
        if String.starts_with?(msg, "Collection '") and
             String.ends_with?(msg, "' already exists") do
          if opts[:yes] do
            Logger.warning(
              "Collection #{name} already exists — importing records into the existing schema."
            )

            case Registry.get(name) do
              {:ok, coll} ->
                # Adopting an existing collection — still sync rules and custom
                # indexes so the target ends up matching the PocketBase source.
                apply_rules(coll, rules)
                DDL.update_collection(name, indexes: indexes)
                Registry.reload!()
                {:ok, coll}

              _ ->
                {:error, "collection not found in registry"}
            end
          else
            {:error,
             "collection already exists (re-run with --yes to import records into the existing schema)"}
          end
        else
          {:error, msg}
        end

      {:error, reason} ->
        {:error, inspect(reason)}
    end
  end

  # PocketBase stores custom indexes as full CREATE INDEX statements, e.g.
  # "CREATE UNIQUE INDEX idx_x ON posts (title, created DESC)". LazyPock
  # stores the bare column expression with an optional UNIQUE prefix and
  # derives its own deterministic index name, so convert between the two.
  defp pb_indexes_to_lazypock(indexes) do
    Enum.map(indexes || [], fn sql ->
      case Regex.run(
             ~r/^\s*CREATE\s+(?:UNIQUE\s+)?INDEX\s+\S+\s+ON\s+\S+\s*\((.*)\)\s*$/is,
             sql
           ) do
        [_, columns] ->
          unique = String.match?(sql, ~r/^\s*CREATE\s+UNIQUE\s+/i)
          if unique, do: "UNIQUE " <> String.trim(columns), else: String.trim(columns)

        _ ->
          Logger.warning(
            "Skipping unrecognized PocketBase index: #{inspect(sql)} (expected a CREATE INDEX statement)"
          )

          nil
      end
    end)
    |> Enum.reject(&is_nil/1)
  end

  # Applies PocketBase rules to a collection struct (DDL's create_collection
  # installs its own defaults first).
  defp apply_rules(collection, rules) do
    {:ok, coll} =
      collection
      |> Lazypock.Collections.Collection.changeset(%{rules: rules})
      |> Repo.update()

    coll
  end

  # ── Field mapping ─────────────────────────────────────

  # PocketBase field types → LazyPock field types. autodate/cast system fields
  # are dropped (LazyPock adds created_at/updated_at itself).
  defp map_field(%{"type" => "autodate"}, _id_to_name), do: nil
  defp map_field(%{"type" => "cast"}, _id_to_name), do: nil

  defp map_field(f, id_to_name) do
    pb_type = f["type"]
    # PocketBase allows camelCase field names (passwordHash, emailVisibility);
    # LazyPock DDL requires lowercase snake_case columns, so we normalize and
    # re-key record values through the same mapping (see name_map in
    # import_records/5).
    name = normalize_field_name(f["name"])

    lazy_type =
      case pb_type do
        "text" ->
          "text"

        "editor" ->
          "editor"

        "email" ->
          "email"

        "url" ->
          "url"

        "number" ->
          "number"

        "bool" ->
          "bool"

        "date" ->
          "date"

        "select" ->
          if multi?(f), do: "multi_select", else: "select"

        "file" ->
          if multi?(f), do: "multi_file", else: "file"

        "relation" ->
          "relation"

        "json" ->
          "json"

        "password" ->
          "password"

        other ->
          Logger.warning("Unknown PocketBase field type '#{other}' — mapping to text")
          "text"
      end

    %{
      "name" => name,
      "type" => lazy_type,
      "required" => !!f["required"],
      "unique" => !!f["unique"],
      "indexed" => !!f["indexed"],
      "hidden" => !!f["hidden"],
      "system" => !!f["system"],
      "options" => map_options(pb_type, f["options"] || %{}, id_to_name)
    }
  end

  defp multi?(f), do: (f["options"] || %{})["maxSelect"] not in [nil, 1]

  defp map_options(pb_type, opts, id_to_name) do
    base =
      case pb_type do
        "file" ->
          %{}
          |> maybe_put("mimeTypes", opts["mimeTypes"])
          |> maybe_put("maxSelect", opts["maxSelect"])
          |> maybe_put("maxFileSize", opts["maxSize"])
          |> maybe_put("thumbs", opts["thumbs"])

        "relation" ->
          %{}
          |> maybe_put("collection", resolve_collection_name(opts["collectionId"], id_to_name))
          |> maybe_put("maxSelect", opts["maxSelect"])
          |> maybe_put("minSelect", opts["minSelect"])
          |> maybe_put("cascadeDelete", opts["cascadeDelete"])

        "select" ->
          %{} |> maybe_put("values", opts["values"]) |> maybe_put("maxSelect", opts["maxSelect"])

        "text" ->
          %{}
          |> maybe_put("max", opts["max"])
          |> maybe_put("min", opts["min"])
          |> maybe_put("pattern", opts["pattern"])

        "number" ->
          %{} |> maybe_put("min", opts["min"]) |> maybe_put("max", opts["max"])

        "date" ->
          %{} |> maybe_put("min", opts["min"]) |> maybe_put("max", opts["max"])

        _ ->
          %{}
      end

    # Thumbs may be a space-joined string in PB ("100x100 400x300") → list
    case base["thumbs"] do
      thumbs when is_binary(thumbs) ->
        Map.put(base, "thumbs", String.split(thumbs, ~r/\s+/))

      _ ->
        base
    end
  end

  # PocketBase relations reference the target by PB collection id; LazyPock
  # relations use the collection name. Resolve via the imported set; if the
  # target collection wasn't imported, warn and keep the id.
  defp resolve_collection_name(collection_id, id_to_name) when is_binary(collection_id) do
    case Map.get(id_to_name, collection_id) do
      nil ->
        Logger.warning(
          "Relation references PocketBase collection id #{collection_id} which is not being imported."
        )

        collection_id

      name ->
        name
    end
  end

  defp resolve_collection_name(_other, _id_to_name), do: nil

  defp maybe_put(map, _key, nil), do: map
  defp maybe_put(map, key, value), do: Map.put(map, key, value)

  # camelCase → snake_case (passwordHash → password_hash, emailVisibility →
  # email_visibility). LazyPock DDL only accepts [a-z0-9_] field names.
  defp normalize_field_name(name) when is_binary(name) do
    name
    |> Macro.underscore()
    |> String.replace(~r/[^a-z0-9_]/, "_")
    |> String.trim("_")
  end

  defp normalize_field_name(nil), do: nil

  # Adds the auth system columns LazyPock's email flows expect. Mirrors the
  # built-in `users` collection migration (20260901000000_add_auth_collection_fields).
  defp ensure_auth_columns!(name) do
    for {col, type, default} <- [
          {"verified", "BOOLEAN", "false"},
          {"email_visibility", "BOOLEAN", "true"}
        ] do
      default_clause = " DEFAULT #{default}"

      Ecto.Adapters.SQL.query!(
        Repo,
        """
        DO $$
        BEGIN
          IF NOT EXISTS (
            SELECT 1 FROM information_schema.columns
            WHERE table_name = '#{name}' AND column_name = '#{col}'
          ) THEN
            ALTER TABLE #{TypeMapper.quote_ident(name)} ADD COLUMN #{col} #{type}#{default_clause};
          END IF;
        END
        $$;
        """
      )

      # Metadata entry so the API exposes the field
      Ecto.Adapters.SQL.query!(
        Repo,
        """
        INSERT INTO _fields (collection_id, name, type, required, system, sort_order, options, created_at, updated_at)
        SELECT c.id, '#{col}', 'bool', true, true, 99, '{}'::jsonb, now(), now()
        FROM _collections c WHERE c.name = '#{name}'
        AND NOT EXISTS (SELECT 1 FROM _fields f WHERE f.collection_id = c.id AND f.name = '#{col}')
        """
      )
    end
  end

  defp pb_rules_to_lazypock(pb_coll) do
    ["listRule", "viewRule", "createRule", "updateRule", "deleteRule", "manageRule"]
    |> Enum.reduce(%{}, fn key, acc ->
      case pb_coll[key] do
        nil -> acc
        "" -> Map.put(acc, key, "")
        value -> Map.put(acc, key, value)
      end
    end)
  end

  # ── Record import ─────────────────────────────────────

  # Old-id → new-uuid map for ALL records across all user collections.
  # Deterministic (uuid5), so it can be built before any record is inserted.
  defp build_id_map(db_path, collections) do
    Enum.reduce(collections, %{}, fn pb_coll, acc ->
      ids =
        query(
          db_path,
          "SELECT id FROM \"#{escape_sql_string(pb_coll["name"])}\""
        )

      Enum.reduce(ids, acc, fn %{"id" => old_id}, acc ->
        Map.put(acc, old_id, uuid5(pb_coll["name"], old_id))
      end)
    end)
  end

  defp import_records(lazy_coll, pb_coll, records, storage_dir, db_path, full_id_map) do
    name = lazy_coll.name
    fields = lazy_coll.fields || []
    file_fields = Enum.filter(fields, &(&1.type in ["file", "multi_file"]))
    relation_fields = Enum.filter(fields, &(&1.type == "relation"))

    # PB field name → LazyPock (normalized) field name, used to re-key record
    # values so camelCase PB columns land in the right snake_case columns.
    name_map =
      Map.new(pb_coll["schema"] || [], fn f -> {f["name"], normalize_field_name(f["name"])} end)

    # Old-id → new-uuid mapping accumulates as we go so later records can
    # rewrite relations pointing to already-imported records.
    Enum.reduce(records, {0, 0, 0, %{}, []}, fn record, {count, files, auths, id_map, warnings} ->
      old_id = record["id"]
      new_id = uuid5(name, old_id)

      record = rekey_record(record, name_map)
      {record, files_copied} = copy_and_rewrite_files(record, pb_coll, file_fields, storage_dir)
      record = rewrite_relations(record, relation_fields, full_id_map)

      case insert_record(lazy_coll, record, new_id) do
        :ok ->
          auths_added = import_external_auths(db_path, pb_coll, record, new_id)

          {count + 1, files + files_copied, auths + auths_added, Map.put(id_map, old_id, new_id),
           warnings}

        {:error, reason} ->
          Logger.error("Failed to insert record #{old_id} into #{name}: #{reason}")
          {count, files, auths, id_map, warnings ++ ["#{name}/#{old_id}: #{reason}"]}
      end
    end)
  end

  defp rekey_record(record, name_map) do
    Map.new(record, fn {k, v} -> {Map.get(name_map, k, k), v} end)
  end

  # Rewrites file field values: PocketBase stores the *filename*; LazyPock
  # stores *file ids*. Copies the binary into LazyPock storage and returns the
  # new value plus how many files were copied.
  defp copy_and_rewrite_files(record, pb_coll, file_fields, storage_dir) do
    Enum.reduce(file_fields, {record, 0}, fn field, {record, count} ->
      raw = record[field.name]

      filenames =
        cond do
          is_list(raw) -> raw
          is_binary(raw) and raw != "" -> [raw]
          true -> []
        end

      {new_values, copied} =
        Enum.reduce(filenames, {[], 0}, fn filename, {acc, copied} ->
          case copy_file(storage_dir, pb_coll["name"], record["id"], filename) do
            {:ok, file_id} -> {[file_id | acc], copied + 1}
            :error -> {acc, copied}
          end
        end)

      new_values = Enum.reverse(new_values)

      record =
        if new_values == [] do
          record
        else
          value = if field.type == "file", do: List.first(new_values), else: new_values
          Map.put(record, field.name, value)
        end

      {record, count + copied}
    end)
  end

  defp copy_file(storage_dir, collection, record_id, filename) do
    # PocketBase layout: storage/<collection>/<recordId>/<filename>
    src = Path.join([storage_dir, collection, record_id, filename])

    if File.exists?(src) do
      binary = File.read!(src)

      case Lazypock.Files.Store.store(binary, filename, []) do
        {:ok, file_record} -> {:ok, file_record["id"]}
        _ -> :error
      end
    else
      Logger.warning("File not found in PocketBase storage: #{src}")
      :error
    end
  end

  defp rewrite_relations(record, relation_fields, id_map) do
    Enum.reduce(relation_fields, record, fn field, record ->
      raw = record[field.name]

      values =
        cond do
          is_list(raw) -> raw
          is_binary(raw) and raw != "" -> [raw]
          true -> []
        end

      new_values = Enum.map(values, fn old -> Map.get(id_map, old, old) end)

      if new_values == [] do
        record
      else
        value =
          if field.type == "relation" and length(new_values) == 1,
            do: hd(new_values),
            else: new_values

        Map.put(record, field.name, value)
      end
    end)
  end

  # Inserts a record with an explicit id (uuidv5 of the PB id) and preserved
  # timestamps. Values are coerced to the column types from the collection
  # schema (PB stores dates as strings, bools as 0/1, etc.).
  defp insert_record(lazy_coll, record, new_id) do
    name = lazy_coll.name
    field_by_name = Map.new(lazy_coll.fields || [], &{&1.name, &1})

    # Only insert columns that exist in the schema (plus id + timestamps and
    # the ensured auth columns).
    auth_extra = if lazy_coll.type == "auth", do: ["verified", "email_visibility"], else: []

    allowed =
      MapSet.new(Map.keys(field_by_name) ++ ["id", "created_at", "updated_at"] ++ auth_extra)

    attrs =
      record
      |> Map.drop([
        "id",
        "created",
        "updated",
        "token_key",
        "last_reset_sent_at",
        "last_verification_sent_at"
      ])
      |> Map.put("id", new_id)
      |> Map.put("created_at", parse_pb_time(record["created"]))
      |> Map.put("updated_at", parse_pb_time(record["updated"]))
      |> Map.filter(fn {k, _v} -> MapSet.member?(allowed, k) end)

    # Auth collections: PB stores verified/emailVisibility as 0/1 integers
    attrs =
      if lazy_coll.type == "auth" do
        attrs
        |> maybe_auth_bool(record, "verified")
        |> maybe_auth_bool(record, "email_visibility")
      else
        attrs
      end

    values =
      Enum.map(attrs, fn {k, v} ->
        if k == "id" do
          # uuid columns need Postgrex's raw 16-byte binary form
          Ecto.UUID.dump!(v)
        else
          coerce_for_column(field_by_name[k], v)
        end
      end)

    columns = Map.keys(attrs)
    placeholders = Enum.map(1..length(columns), &"$#{&1}")

    sql = """
    INSERT INTO #{TypeMapper.quote_ident(name)} (#{Enum.map_join(columns, ", ", &TypeMapper.quote_ident/1)})
    VALUES (#{Enum.join(placeholders, ", ")})
    ON CONFLICT (id) DO NOTHING
    """

    case Ecto.Adapters.SQL.query(Repo, sql, values) do
      {:ok, _} -> :ok
      {:error, err} -> {:error, Exception.message(err)}
    end
  end

  # Sets `lazy_name` from the raw PB record value (0/1 integers) when present.
  defp maybe_auth_bool(attrs, record, lazy_name) do
    case Map.get(record, lazy_name) do
      nil -> attrs
      value -> Map.put(attrs, lazy_name, value == true or value == 1)
    end
  end

  # Coerce a raw PocketBase value to what Postgrex expects for the column.
  defp coerce_for_column(nil, value), do: value

  defp coerce_for_column(%{type: t}, value) when t in ["date", "datetime"],
    do: parse_pb_time(value)

  defp coerce_for_column(%{type: "bool"}, value), do: value == true or value == 1

  defp coerce_for_column(%{type: "json"}, value) when is_map(value) or is_list(value),
    do: Jason.encode!(value)

  defp coerce_for_column(%{type: "json"}, value), do: value
  defp coerce_for_column(%{type: "number"}, value) when is_binary(value), do: parse_number(value)
  defp coerce_for_column(_field, value), do: value

  defp parse_number(""), do: nil

  defp parse_number(v) when is_binary(v) do
    case Float.parse(v) do
      {f, ""} ->
        f

      _ ->
        case Integer.parse(v) do
          {i, ""} -> i
          _ -> v
        end
    end
  end

  defp import_external_auths(db_path, pb_coll, record, new_id) do
    old_id = record["id"]
    escaped = escape_sql_string(old_id)

    db_path
    |> query("SELECT * FROM _externalAuths WHERE recordRef = '#{escaped}'")
    |> Enum.reduce(0, fn row, acc ->
      insert_external_auth(pb_coll, row, new_id)
      acc + 1
    end)
  end

  defp insert_external_auth(pb_coll, row, new_id) do
    Ecto.Adapters.SQL.query!(
      Repo,
      """
      INSERT INTO _external_auths (collection, provider, provider_id, user_id, created_at, updated_at)
      VALUES ($1, $2, $3, $4, $5, $6)
      ON CONFLICT (collection, provider, provider_id) DO NOTHING
      """,
      [
        pb_coll["name"],
        row["provider"],
        row["providerId"],
        Ecto.UUID.dump!(new_id),
        parse_pb_time(row["created"]) || DateTime.utc_now(),
        parse_pb_time(row["updated"]) || DateTime.utc_now()
      ]
    )

    :ok
  end

  # ── Helpers ───────────────────────────────────────────

  @doc "Deterministic UUIDv5 from the PocketBase collection + record id."
  @spec uuid5(String.t(), String.t()) :: String.t()
  def uuid5(collection, old_id) do
    # SHA-1 is 160 bits; UUIDv5 uses the first 128 bits with version/variant bits.
    digest = :crypto.hash(:sha, "#{collection}##{old_id}")
    <<a::32, b::16, c::16, d::16, e::48, _rest::binary>> = digest
    c = (c &&& 0x0FFF) ||| 0x5000
    d = (d &&& 0x3FFF) ||| 0x8000

    :io_lib.format("~8.16.0b-~4.16.0b-~4.16.0b-~4.16.0b-~12.16.0b", [a, b, c, d, e])
    |> IO.iodata_to_binary()
  end

  defp parse_pb_time(nil), do: nil

  defp parse_pb_time(value) when is_binary(value) do
    case DateTime.from_iso8601(value) do
      {:ok, dt, _} -> dt
      _ -> nil
    end
  end

  defp parse_pb_time(%DateTime{} = dt), do: dt
  defp parse_pb_time(_), do: nil

  defp escape_sql_string(value) when is_binary(value),
    do: String.replace(value, "'", "''")

  defp escape_sql_string(value), do: to_string(value)
end
