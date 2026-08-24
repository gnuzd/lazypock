defmodule Lazypock.Schema.DDL do
  @moduledoc """
  Executes DDL operations safely using PostgreSQL's transactional DDL.

  All DDL operations run inside `Repo.transaction` — if anything fails,
  the entire transaction (including DDL) rolls back automatically.
  PubSub broadcasts happen AFTER the commit so the Registry
  (which connects via a different DB connection) sees committed data.

  Broadcasts are safe when `Lazypock.PubSub` isn't running (e.g. the
  `lazypock migrate` CLI or a boot-time user migration) — they become a
  no-op and the registry simply picks the change up from the DB on boot.
  """

  alias Lazypock.Repo
  alias Lazypock.Schema.TypeMapper
  import Ecto.Query

  # Broadcast a schema event to the in-process Registry, unless PubSub isn't
  # started yet (CLI migrate / boot-time migrations). The Registry reloads
  # from the DB on startup, so a skipped broadcast is always reconciled.
  defp safe_broadcast(topic, message) do
    if Process.whereis(Lazypock.PubSub) do
      Phoenix.PubSub.broadcast(Lazypock.PubSub, topic, message)
    end

    :ok
  end

  # ── Create collection ──────────────────────────────

  @doc """
  Creates a new collection table and stores its metadata.

  ## Options

    * `:type` - Collection type: `"base"` (default) or `"auth"`
    * `:fields` - List of field definition maps (see README for format)
    * `:indexes` - List of custom index expressions (e.g. `["UNIQUE email", "created_at DESC"]`)
  """
  @spec create_collection(String.t(), keyword()) ::
          {:ok, Lazypock.Collections.Collection.t()} | {:error, term()}
  def create_collection(name, opts \\ []) when is_binary(name) do
    type = Keyword.get(opts, :type, "base")
    fields = Keyword.get(opts, :fields, [])
    indexes = Keyword.get(opts, :indexes, [])
    rules = Keyword.get(opts, :rules)
    options = Keyword.get(opts, :options)
    hooks = Keyword.get(opts, :hooks)

    result =
      Repo.transaction(fn ->
        with :ok <- validate_collection_name(name),
             :ok <- validate_collection_not_exists(name),
             :ok <- validate_fields(fields) do
          lock_key = :erlang.phash2({:create_collection, name})
          Ecto.Adapters.SQL.query!(Repo, "SELECT pg_advisory_xact_lock(#{lock_key})", [])

          collection =
            create_collection_metadata!(name, type, fields, indexes, rules, options, hooks)

          sql = build_create_table_sql(name, fields)
          Ecto.Adapters.SQL.query!(Repo, sql, [])

          create_indexes(name, fields)
          apply_custom_indexes!(name, indexes)
          create_field_metadata!(collection.id, fields)

          Repo.preload(collection, :fields)
        else
          # Validation failures must roll back so the transaction returns
          # {:error, reason} — otherwise the caller would receive the
          # double-wrapped {:ok, {:error, reason}} and treat a failure as
          # success (and broadcast a malformed message to the Registry).
          {:error, reason} -> Repo.rollback(reason)
        end
      end)

    case result do
      {:ok, collection} ->
        safe_broadcast("schema", {:collection_created, collection})
        {:ok, collection}

      {:error, reason} ->
        {:error, reason}
    end
  end

  # ── Add field ──────────────────────────────────────

  @doc """
  Adds a new field (column) to an existing collection table.
  """
  @spec add_field(String.t(), map()) :: :ok | {:error, term()}
  def add_field(collection_name, field_def) do
    result =
      Repo.transaction(fn ->
        with :ok <- validate_field_name!(field_def["name"]),
             :ok <- validate_field_type(field_def["type"]) do
          lock_key = :erlang.phash2({:add_field, collection_name, field_def["name"]})
          Ecto.Adapters.SQL.query!(Repo, "SELECT pg_advisory_xact_lock(#{lock_key})", [])

          {:ok, collection} = get_collection(collection_name)

          sql = """
          ALTER TABLE #{TypeMapper.quote_ident(collection_name)}
          ADD COLUMN #{TypeMapper.quote_ident(column_name(field_def["name"]))}
          #{TypeMapper.to_pg_with_opts(field_def["type"], field_def["options"] || %{})}
          #{if field_def["required"], do: " NOT NULL", else: ""}
          #{TypeMapper.default_sql(field_def)}
          """

          Ecto.Adapters.SQL.query!(Repo, sql, [])

          if field_def["indexed"] do
            create_index(collection_name, field_def["name"])
          end

          if field_def["unique"] do
            create_unique_index(collection_name, field_def["name"])
          end

          create_field_metadata_entry!(collection.id, field_def)
          update_collection_schema!(collection)

          :ok
        end
      end)

    case result do
      {:ok, :ok} ->
        safe_broadcast("schema", {:field_added, collection_name, field_def})

        :ok

      {:ok, {:error, reason}} ->
        {:error, reason}

      {:error, reason} ->
        {:error, reason}

      other ->
        other
    end
  end

  # ── Drop field ─────────────────────────────────────

  @doc """
  Removes a field (column) from an existing collection table.
  """
  @spec drop_field(String.t(), String.t()) :: :ok | {:error, term()}
  def drop_field(collection_name, field_name) do
    result =
      Repo.transaction(fn ->
        {:ok, collection} = get_collection(collection_name)

        Ecto.Adapters.SQL.query!(
          Repo,
          "ALTER TABLE #{TypeMapper.quote_ident(collection_name)} DROP COLUMN IF EXISTS #{TypeMapper.quote_ident(field_name)} CASCADE",
          []
        )

        Repo.delete_all(
          from(f in Lazypock.Collections.Field,
            join: c in assoc(f, :collection),
            where: c.name == ^collection_name and f.name == ^field_name
          )
        )

        update_collection_schema!(collection)

        :ok
      end)

    case result do
      {:ok, :ok} ->
        safe_broadcast("schema", {:field_removed, collection_name, field_name})

        :ok

      {:ok, {:error, reason}} ->
        {:error, reason}

      {:error, reason} ->
        {:error, reason}

      other ->
        other
    end
  end

  # ── Update collection ──────────────────────────────

  @doc """
  Updates a collection: renames the table, changes type, adds/removes fields.

  Accepts `:indexes` (list of custom index expressions). When provided,
  custom DB indexes are diffed against the stored list and synced.
  """
  @spec update_collection(String.t(), keyword()) :: {:ok, map()} | {:error, term()}
  def update_collection(old_name, opts \\ []) when is_binary(old_name) do
    new_name = Keyword.get(opts, :name, old_name)
    type = Keyword.get(opts, :type)
    # nil when not provided — omitting :fields must NOT drop all columns
    new_fields = Keyword.get(opts, :fields)
    new_indexes = Keyword.get(opts, :indexes)
    # When false (e.g. import with "delete missing" disabled), fields absent
    # from the payload are left untouched. Defaults to true so the collection
    # editor (which always sends the full field list) keeps its delete-on-remove
    # behavior. System fields are NEVER dropped, regardless of this flag.
    delete_missing_fields = Keyword.get(opts, :delete_missing_fields, true)

    result =
      Repo.transaction(fn ->
        {:ok, collection} = get_collection(old_name)

        # System collections cannot be renamed
        if collection.system and new_name != old_name do
          Repo.rollback("Cannot rename system collection '#{old_name}'")
        else
          lock_key = :erlang.phash2({:update_collection, old_name})
          Ecto.Adapters.SQL.query!(Repo, "SELECT pg_advisory_xact_lock(#{lock_key})", [])

          if new_name != old_name do
            Ecto.Adapters.SQL.query!(
              Repo,
              "ALTER TABLE #{TypeMapper.quote_ident(old_name)} RENAME TO #{TypeMapper.quote_ident(new_name)}",
              []
            )

            Repo.update_all(
              from(c in Lazypock.Collections.Collection, where: c.id == ^collection.id),
              set: [name: new_name]
            )
          end

          if type do
            Repo.update_all(
              from(c in Lazypock.Collections.Collection, where: c.id == ^collection.id),
              set: [type: type]
            )
          end

          # Save metadata fields if provided (rules, options, hooks)
          metadata = [:rules, :options, :hooks]

          metadata_updates =
            metadata
            |> Enum.reduce(%{}, fn key, acc ->
              case Keyword.fetch(opts, key) do
                {:ok, value} when is_map(value) ->
                  Map.put(acc, key, value)

                {:ok, value} ->
                  Map.put(acc, key, value)

                :error ->
                  acc
              end
            end)

          unless metadata_updates == %{} do
            collection
            |> Ecto.Changeset.change(metadata_updates)
            |> Repo.update!()
          end

          # Sync custom indexes (options["indexes"]) with actual DB indexes.
          if not is_nil(new_indexes) do
            # Base the merge on the incoming :options when provided (e.g. an
            # export→import round-trip) so other option keys from the payload
            # aren't clobbered by the stale pre-update collection struct.
            old_options =
              case Keyword.fetch(opts, :options) do
                {:ok, value} when is_map(value) -> value
                _ -> collection.options || %{}
              end

            old_indexes = Map.get(old_options, "indexes", []) || []

            sync_custom_indexes!(new_name, old_indexes, new_indexes)

            # Persist the new list in options (merged with any other option keys).
            merged_opts = Map.put(old_options, "indexes", new_indexes)

            collection
            |> Ecto.Changeset.change(%{options: merged_opts})
            |> Repo.update!()
          end

          # Sync fields ONLY when explicitly provided. Omitting :fields (e.g. a
          # bare rename or rules-only update) must NOT silently drop every column:
          # the controller sends the full field list on schema edits, and
          # anything else is a partial update that should leave columns alone.
          if not is_nil(new_fields) do
            existing_fields =
              Repo.all(
                from(f in Lazypock.Collections.Field, where: f.collection_id == ^collection.id)
              )

            existing_names = MapSet.new(existing_fields, & &1.name)
            incoming_names = MapSet.new(new_fields, & &1["name"])

            # Remove deleted fields (only when deletion is requested; system
            # fields are never removed — e.g. verified/email_visibility on the
            # auth users collection).
            for f <- existing_fields,
                not MapSet.member?(incoming_names, f.name),
                delete_missing_fields,
                not f.system do
              Ecto.Adapters.SQL.query!(
                Repo,
                "ALTER TABLE #{TypeMapper.quote_ident(new_name)} DROP COLUMN IF EXISTS #{TypeMapper.quote_ident(f.name)} CASCADE",
                []
              )

              Repo.delete_all(
                from(fld in Lazypock.Collections.Field,
                  where: fld.collection_id == ^collection.id and fld.name == ^f.name
                )
              )
            end

            # Update existing fields with new options/properties
            # Derive sort_order from array position so drag-reorder works
            fields_with_sort =
              new_fields
              |> Enum.with_index()
              |> Enum.map(fn {f, idx} ->
                Map.put(f, "sort_order", idx)
              end)

            for f <- fields_with_sort, MapSet.member?(existing_names, f["name"]) do
              opts =
                if f["type"] == "relation" do
                  normalize_relation_opts(f)
                else
                  Map.get(f, "options", %{})
                end

              old = Enum.find(existing_fields, &(&1.name == f["name"]))
              new_unique = Map.get(f, "unique", false)
              new_indexed = Map.get(f, "indexed", false)

              # Sync actual DB indexes with the requested unique/indexed flags:
              # drop the old index(es) when the flags change, then recreate.
              if old do
                if old.unique != new_unique or old.indexed != new_indexed do
                  drop_index_if_exists(new_name, f["name"])
                end

                if new_indexed, do: create_index(new_name, f["name"])
                if new_unique, do: create_unique_index(new_name, f["name"])
              end

              Repo.update_all(
                from(fld in Lazypock.Collections.Field,
                  where: fld.collection_id == ^collection.id and fld.name == ^f["name"]
                ),
                set: [
                  required: Map.get(f, "required", false),
                  unique: new_unique,
                  hidden: Map.get(f, "hidden", false),
                  system: Map.get(f, "system", false),
                  options: opts,
                  indexed: new_indexed,
                  sort_order: f["sort_order"]
                ]
              )
            end

            # Add new fields (also derive sort_order from position)
            for f <- fields_with_sort, not MapSet.member?(existing_names, f["name"]) do
              :ok = validate_field_name!(f["name"])
              :ok = validate_field_type(f["type"])

              sql =
                "ALTER TABLE #{TypeMapper.quote_ident(new_name)} ADD COLUMN #{TypeMapper.quote_ident(column_name(f["name"]))} #{TypeMapper.to_pg_with_opts(f["type"], f["options"] || %{})} #{if f["required"], do: " NOT NULL", else: ""} #{TypeMapper.default_sql(f)}"

              Ecto.Adapters.SQL.query!(Repo, sql, [])
              if f["indexed"], do: create_index(new_name, f["name"])
              if f["unique"], do: create_unique_index(new_name, f["name"])
              create_field_metadata_entry!(collection.id, f)
            end
          end

          collection_refresh =
            Repo.get!(Lazypock.Collections.Collection, collection.id) |> Repo.preload(:fields)

          update_collection_schema!(collection_refresh)
          collection_refresh
        end
      end)

    case result do
      {:ok, collection} ->
        safe_broadcast("schema", {:collection_updated, collection})
        {:ok, collection}

      {:error, reason} ->
        {:error, reason}

      other ->
        other
    end
  end

  # ── Drop collection ────────────────────────────────

  @doc """
  Drops a collection table completely (only if `managed` is true).
  """
  @spec drop_collection(String.t()) :: :ok | {:error, term()}
  def drop_collection(name) do
    case get_collection(name) do
      {:ok, collection} -> do_drop(collection)
      {:error, reason} -> {:error, reason}
    end
  end

  defp do_drop(collection) do
    result =
      Repo.transaction(fn ->
        cond do
          # Protect by the DB flag only. The built-in users collection is a
          # normal auth collection (matching PocketBase) and is deliberately
          # not protected — like in PocketBase, it can be renamed/deleted.
          collection.system ->
            {:error, "Cannot delete system collection '#{collection.name}'"}

          collection.managed ->
            Ecto.Adapters.SQL.query!(
              Repo,
              "DROP TABLE IF EXISTS #{TypeMapper.quote_ident(collection.name)} CASCADE",
              []
            )

            Repo.delete!(collection)
            :ok

          true ->
            {:error, :not_managed}
        end
      end)

    case result do
      {:ok, :ok} ->
        safe_broadcast("schema", {:collection_deleted, collection.name})

        :ok

      {:ok, {:error, reason}} ->
        {:error, reason}

      {:error, reason} ->
        {:error, reason}

      other ->
        other
    end
  end

  # ── Private helpers ──

  defp validate_collection_name(name) do
    if name =~ ~r/^[a-z][a-z0-9_]*$/ do
      :ok
    else
      {:error,
       "Collection name must start with a letter and contain only lowercase letters, numbers, and underscores"}
    end
  end

  defp validate_collection_not_exists(name) do
    case Repo.get_by(Lazypock.Collections.Collection, name: name) do
      nil -> :ok
      _ -> {:error, "Collection '#{name}' already exists"}
    end
  end

  defp validate_fields(fields) do
    names = Enum.map(fields, & &1["name"])

    cond do
      Enum.any?(names, &(not (&1 =~ ~r/^[A-Za-z][A-Za-z0-9_]*$/))) ->
        {:error,
         "Field names must start with a letter and contain only letters, numbers, and underscores"}

      length(Enum.uniq(names)) != length(names) ->
        {:error, "Duplicate field names are not allowed"}

      Enum.any?(fields, &(not TypeMapper.valid_type?(&1["type"]))) ->
        {:error, "Invalid field type"}

      true ->
        :ok
    end
  end

  defp validate_field_name!(field_name) do
    # Mixed case is allowed (e.g. `tagColor`) — the name is kept verbatim as
    # the metadata/API name; the DB column is derived (lowercased) and bridged
    # by Lazypock.Schemas.FieldNames on reads/writes.
    if field_name =~ ~r/^[A-Za-z][A-Za-z0-9_]*$/ do
      :ok
    else
      {:error,
       "Field name must start with a letter and contain only letters, numbers, and underscores"}
    end
  end

  # DB column for a field name: the field name is kept verbatim as the
  # metadata/API name (e.g. `tagColor`); the Postgres column is its lowercase
  # form (e.g. `tagcolor`), matching how the system migrations create columns
  # and what Lazypock.Schemas.FieldNames bridges on reads/writes.
  defp column_name(name) when is_binary(name), do: String.downcase(name)
  defp column_name(name), do: name

  defp validate_field_type(type) do
    if TypeMapper.valid_type?(type), do: :ok, else: {:error, "Invalid field type: #{type}"}
  end

  defp build_create_table_sql(name, fields) do
    columns = Enum.map(fields, &column_def/1)
    pk = "id UUID PRIMARY KEY DEFAULT gen_random_uuid()"

    ts =
      "created_at TIMESTAMPTZ NOT NULL DEFAULT now(), updated_at TIMESTAMPTZ NOT NULL DEFAULT now()"

    all_cols =
      if columns == [] do
        "#{pk},\n      #{ts}"
      else
        "#{pk},\n      #{Enum.join(columns, ",\n      ")},\n      #{ts}"
      end

    """
    CREATE TABLE #{TypeMapper.quote_ident(name)} (
      #{all_cols}
    )
    """
  end

  defp column_def(field) do
    col = TypeMapper.quote_ident(column_name(field["name"]))
    pg_type = TypeMapper.to_pg_with_opts(field["type"], field["options"] || %{})
    not_null = if field["required"], do: " NOT NULL", else: ""
    default = TypeMapper.default_sql(field)
    "#{col} #{pg_type}#{not_null} #{default}" |> String.trim_trailing()
  end

  defp create_indexes(name, fields) do
    Enum.each(fields, fn field ->
      if field["indexed"], do: create_index(name, field["name"])
      if field["unique"], do: create_unique_index(name, field["name"])
    end)
  end

  defp create_index(table, column) do
    column = column_name(column)
    index_name = "#{table}_#{column}_idx"

    Ecto.Adapters.SQL.query!(
      Repo,
      "CREATE INDEX IF NOT EXISTS #{TypeMapper.quote_ident(index_name)} ON #{TypeMapper.quote_ident(table)} (#{TypeMapper.quote_ident(column)})",
      []
    )
  end

  defp create_unique_index(table, column) do
    column = column_name(column)
    index_name = "#{table}_#{column}_unq"

    Ecto.Adapters.SQL.query!(
      Repo,
      "CREATE UNIQUE INDEX IF NOT EXISTS #{TypeMapper.quote_ident(index_name)} ON #{TypeMapper.quote_ident(table)} (#{TypeMapper.quote_ident(column)})",
      []
    )
  end

  defp drop_index_if_exists(table, column) do
    for suffix <- ["_idx", "_unq"] do
      index_name = "#{table}_#{column}#{suffix}"

      Ecto.Adapters.SQL.query!(
        Repo,
        "DROP INDEX IF EXISTS #{TypeMapper.quote_ident(index_name)}",
        []
      )
    end
  end

  # ── Custom (multi-column) indexes ────────────────────

  # Custom indexes are expressions like "UNIQUE email" or "created_at DESC, id"
  # stored in the collection's options["indexes"] as a list of strings.
  # A deterministic index name is derived from the expression so we can
  # drop-and-recreate when the list changes.
  defp apply_custom_indexes!(table, indexes) do
    Enum.each(indexes || [], fn expr ->
      index_name = custom_index_name(expr)
      {unique, columns} = parse_custom_index_expr(expr)

      sql =
        "CREATE #{if unique, do: "UNIQUE ", else: ""}INDEX IF NOT EXISTS #{TypeMapper.quote_ident(index_name)} ON #{TypeMapper.quote_ident(table)} (#{columns})"

      Ecto.Adapters.SQL.query!(Repo, sql, [])
    end)
  end

  defp drop_custom_index!(_table, expr) do
    index_name = custom_index_name(expr)

    Ecto.Adapters.SQL.query!(
      Repo,
      "DROP INDEX IF EXISTS #{TypeMapper.quote_ident(index_name)}",
      []
    )
  end

  defp custom_index_name(expr) do
    hash =
      expr
      |> String.replace(~r/\s+/, "")
      |> :erlang.phash2()

    "_cx_#{:erlang.integer_to_binary(hash) |> String.replace("-", "n")}"
  end

  # "UNIQUE col1, col2" -> {true, "\"col1\", \"col2\""}
  # "col1, col2 DESC"   -> {false, "\"col1\", \"col2\" DESC"}
  defp parse_custom_index_expr(expr) do
    {unique, rest} =
      case String.trim(expr) do
        "UNIQUE " <> cols -> {true, cols}
        other -> {false, other}
      end

    columns =
      rest
      |> String.split(",")
      |> Enum.map(&String.trim/1)
      |> Enum.map(fn col ->
        case String.split(col, ~r/\s+/, parts: 2) do
          [name, suffix] -> "#{TypeMapper.quote_ident(name)} #{suffix}"
          [name] -> TypeMapper.quote_ident(name)
        end
      end)
      |> Enum.join(", ")

    {unique, columns}
  end

  # Drop all custom indexes not in the new list (used by update_collection).
  defp sync_custom_indexes!(table, old_indexes, new_indexes) do
    new_set = MapSet.new(new_indexes || [])

    for expr <- old_indexes || [], not MapSet.member?(new_set, expr) do
      drop_custom_index!(table, expr)
    end

    apply_custom_indexes!(table, new_indexes)
  end

  defp create_collection_metadata!(name, type, fields, indexes, rules, options, hooks) do
    initial_schema =
      Enum.map(fields, fn f ->
        %{
          "name" => f["name"],
          "type" => f["type"],
          "required" => Map.get(f, "required", false),
          "unique" => Map.get(f, "unique", false),
          "default" => Map.get(f, "default"),
          "options" => Map.get(f, "options", %{}),
          "indexed" => Map.get(f, "indexed", false)
        }
      end)

    # Default rules for auth collections (matching PocketBase auth defaults)
    default_rules =
      if type == "auth" do
        %{
          "listRule" => "",
          "viewRule" => "",
          "createRule" => "",
          "updateRule" => "id = @request.auth.id",
          "deleteRule" => "id = @request.auth.id",
          "manageRule" => nil
        }
      else
        %{
          "listRule" => "",
          "viewRule" => "",
          "createRule" => "@request.auth.id != ''",
          "updateRule" => "",
          "deleteRule" => "",
          "manageRule" => nil
        }
      end

    # Explicitly-applied indexes always win; other option keys (if any) come
    # from the payload so an export→import round-trip preserves them.
    options = Map.merge(options || %{}, %{"indexes" => indexes || []})

    %Lazypock.Collections.Collection{}
    |> Lazypock.Collections.Collection.changeset(%{
      name: name,
      type: type,
      schema: initial_schema,
      rules: rules || default_rules,
      options: options,
      hooks: hooks || %{},
      managed: true
    })
    |> Repo.insert!()
  end

  defp create_field_metadata!(collection_id, fields) do
    Enum.each(fields, fn field_def ->
      create_field_metadata_entry!(collection_id, field_def)
    end)
  end

  defp create_field_metadata_entry!(collection_id, field_def) do
    normalized_opts =
      if field_def["type"] == "relation" do
        normalize_relation_opts(field_def)
      else
        Map.get(field_def, "options", %{})
      end

    %Lazypock.Collections.Field{}
    |> Lazypock.Collections.Field.changeset(%{
      collection_id: collection_id,
      name: field_def["name"],
      type: field_def["type"],
      required: Map.get(field_def, "required", false),
      unique: Map.get(field_def, "unique", false),
      default_value: field_def["default"],
      options: normalized_opts,
      indexed: Map.get(field_def, "indexed", false),
      sort_order: Map.get(field_def, "sort_order", 0)
    })
    |> Repo.insert!()
  end

  defp normalize_relation_opts(field_def) do
    opts = Map.get(field_def, "options", %{})
    # If options already has a "collection" key pointing to a name, use it
    if is_binary(opts["collection"]) and opts["collection"] != "" do
      opts
    else
      # Try to resolve collectionId (UUID from SPA, or collection name) to a
      # collection name. The id column is :binary_id, so only query by id when
      # the value is actually a UUID — a PocketBase collection id (e.g.
      # "kanban_columns_col") or a bare name would otherwise crash the query.
      raw_id = field_def["collectionId"] || opts["collectionId"]

      if is_binary(raw_id) and raw_id != "" do
        collection =
          if match?({:ok, _}, Ecto.UUID.cast(raw_id)) do
            Lazypock.Repo.get_by(Lazypock.Collections.Collection, id: raw_id) ||
              Lazypock.Repo.get_by(Lazypock.Collections.Collection, name: raw_id)
          else
            Lazypock.Repo.get_by(Lazypock.Collections.Collection, name: raw_id)
          end

        if collection do
          Map.put(opts, "collection", collection.name)
        else
          opts
        end
      else
        opts
      end
    end
  end

  defp update_collection_schema!(collection) do
    fields =
      Repo.all(
        from(f in Lazypock.Collections.Field,
          where: f.collection_id == ^collection.id,
          order_by: f.sort_order
        )
      )

    schema =
      Enum.map(fields, fn f ->
        %{
          "name" => f.name,
          "type" => f.type,
          "required" => f.required,
          "unique" => f.unique,
          "default" => f.default_value,
          "options" => f.options,
          "indexed" => f.indexed
        }
      end)

    Repo.update_all(
      from(c in Lazypock.Collections.Collection, where: c.id == ^collection.id),
      set: [schema: schema]
    )
  end

  defp get_collection(name) do
    case Repo.get_by(Lazypock.Collections.Collection, name: name) do
      nil -> {:error, :not_found}
      collection -> {:ok, collection}
    end
  end
end
