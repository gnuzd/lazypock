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
  alias Lazypock.Schema.Views
  require Logger
  import Ecto.Query

  # Broadcast a schema event to the in-process Registry, unless PubSub isn't
  # started yet (CLI migrate / boot-time migrations). The Registry reloads
  # from the DB on startup, so a skipped broadcast is always reconciled.
  #
  # Under test the Ecto sandbox ties DB connections to the test process, and
  # the Registry applies schema broadcasts with its own DB queries. If one is
  # still in flight when the test exits, Postgrex logs "owner exited"
  # disconnect noise. We therefore also apply the event synchronously (a
  # GenServer.call) so the registry's cache is updated before the DDL call
  # returns. No-op in production (the PubSub delivery above already keeps the
  # registry in sync, asynchronously) and whenever the registry isn't running
  # (lazypock migrate CLI / boot-time migrations).
  defp safe_broadcast(topic, message) do
    if Process.whereis(Lazypock.PubSub) do
      Phoenix.PubSub.broadcast(Lazypock.PubSub, topic, message)
    end

    if Code.ensure_loaded?(ExUnit) do
      case Process.whereis(Lazypock.Collections.Registry) do
        nil -> :ok
        _pid -> GenServer.call(Lazypock.Collections.Registry, {:apply_event, message})
      end
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
             :ok <- validate_collection_not_exists(name) do
          lock_key = :erlang.phash2({:create_collection, name})
          Ecto.Adapters.SQL.query!(Repo, "SELECT pg_advisory_xact_lock(#{lock_key})", [])

          if type == "view" do
            # View collections: schema is derived from the view query, no table.
            create_view_collection!(name, opts)
          else
            with :ok <- validate_fields(fields) do
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
          end
        else
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

  # ── View collections ────────────────────────────────

  # Creates a view collection: validates the query, derives the fields from
  # its result columns, creates the physical Postgres view, and stores the
  # metadata. Mirrors PocketBase's view-collection save flow.
  defp create_view_collection!(name, opts) do
    options = normalize_view_options(Keyword.get(opts, :options) || %{}, opts)
    query = Map.get(options, "view_query")

    cond do
      not is_binary(query) or query == "" ->
        Repo.rollback("view query is required")

      true ->
        case Views.build_fields(query) do
          {:ok, fields} ->
            collection =
              create_collection_metadata!(
                name,
                "view",
                fields,
                [],
                opts[:rules],
                options,
                opts[:hooks]
              )

            case Views.create_view(name, query) do
              {:ok, _columns} ->
                create_field_metadata!(collection.id, fields)
                Lazypock.Realtime.Views.reset_view(name)
                Repo.preload(collection, :fields)

              {:error, reason} ->
                Repo.rollback(reason)
            end

          {:error, reason} ->
            Repo.rollback(reason)
        end
    end
  end

  # Merges a top-level :view_query keyword (internal callers) into the
  # options map so options["view_query"] is the single source of truth.
  defp normalize_view_options(options, opts) do
    case Keyword.get(opts, :view_query) do
      nil -> options
      query -> Map.put(options, "view_query", query)
    end
  end

  # Updates a view collection. The schema is derived from the view query, so
  # field payloads are rejected; the physical view is rebuilt (and fields
  # re-detected) whenever the query changes or the collection is renamed.
  # Mirrors PocketBase's view-collection save flow.
  defp update_view_collection!(old_name, collection, opts) do
    new_name = Keyword.get(opts, :name, old_name)

    cond do
      collection.system and new_name != old_name ->
        Repo.rollback("Cannot rename system collection '#{old_name}'")

      Keyword.has_key?(opts, :type) and Keyword.get(opts, :type) != "view" ->
        Repo.rollback("Cannot change the type of a view collection")

      Keyword.has_key?(opts, :fields) ->
        Repo.rollback("View collection fields are auto-generated from the view query")

      true ->
        lock_key = :erlang.phash2({:update_collection, old_name})
        Ecto.Adapters.SQL.query!(Repo, "SELECT pg_advisory_xact_lock(#{lock_key})", [])

        old_options = collection.options || %{}
        new_options = Keyword.get(opts, :options, old_options) || old_options
        old_query = Map.get(old_options, "view_query")
        new_query = Map.get(new_options, "view_query")
        effective_query = new_query || old_query
        query_changed = is_binary(new_query) and new_query != old_query
        renamed = new_name != old_name

        # Rebuild the physical view + fields only when the query changed or
        # on rename (rules/options-only updates leave the view untouched).
        if query_changed or renamed do
          if not is_binary(effective_query) or effective_query == "" do
            Repo.rollback("view query is required")
          end

          case Views.build_fields(effective_query) do
            {:ok, fields} ->
              if renamed do
                Views.drop_view(old_name)
              end

              case Views.create_view(new_name, effective_query) do
                {:ok, _columns} ->
                  if renamed do
                    Repo.update_all(
                      from(c in Lazypock.Collections.Collection, where: c.id == ^collection.id),
                      set: [name: new_name]
                    )
                  end

                  Repo.delete_all(
                    from(f in Lazypock.Collections.Field,
                      where: f.collection_id == ^collection.id
                    )
                  )

                  create_field_metadata!(collection.id, fields)
                  Lazypock.Realtime.Views.reset_view(new_name)
                  if renamed, do: Lazypock.Realtime.Views.reset_view(old_name)

                {:error, reason} ->
                  Repo.rollback(reason)
              end

            {:error, reason} ->
              Repo.rollback(reason)
          end
        end

        # Save metadata (rules/options/hooks) — mirrors the base path.
        metadata = [:rules, :options, :hooks]

        metadata_updates =
          metadata
          |> Enum.reduce(%{}, fn key, acc ->
            case Keyword.fetch(opts, key) do
              {:ok, value} -> Map.put(acc, key, value)
              :error -> acc
            end
          end)

        unless metadata_updates == %{} do
          collection
          |> Ecto.Changeset.change(metadata_updates)
          |> Repo.update!()
        end

        collection_refresh =
          Repo.get!(Lazypock.Collections.Collection, collection.id) |> Repo.preload(:fields)

        update_collection_schema!(collection_refresh)
        collection_refresh
    end
  end

  @doc """
  Re-saves all view collections except `exclude_name`, mirroring PocketBase's
  `resaveViewsWithChangedFields`: after a base/auth collection is saved, its
  renamed/retyped columns can drift view field metadata, so every view is
  re-introspected and its physical view rebuilt.

  Errors are logged, never raised — a problematic view query must be fixed
  from the UI (PocketBase behavior).
  """
  @spec sync_dependent_views(String.t()) :: :ok
  def sync_dependent_views(exclude_name) do
    views =
      Repo.all(from(c in Lazypock.Collections.Collection, where: c.type == "view"))
      |> Enum.reject(&(&1.name == exclude_name))
      |> Repo.preload(:fields)

    Enum.each(views, fn view ->
      query = Map.get(view.options || %{}, "view_query")

      if is_binary(query) and query != "" do
        try do
          Repo.transaction(fn ->
            case Views.build_fields(query) do
              {:ok, fields} ->
                case Views.create_view(view.name, query) do
                  {:ok, _columns} ->
                    Repo.delete_all(
                      from(f in Lazypock.Collections.Field, where: f.collection_id == ^view.id)
                    )

                    create_field_metadata!(view.id, fields)
                    update_collection_schema!(view)
                    Lazypock.Realtime.Views.reset_view(view.name)

                  {:error, reason} ->
                    Repo.rollback(reason)
                end

              {:error, reason} ->
                Repo.rollback(reason)
            end
          end)
        rescue
          e -> Logger.error("Failed to sync view '#{view.name}': #{Exception.message(e)}")
        end
      end
    end)

    :ok
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

        cond do
          collection.type == "view" ->
            # View collections have their own update path (query-driven fields).
            update_view_collection!(old_name, collection, opts)

          type == "view" ->
            Repo.rollback(
              "Converting an existing collection to a view is not supported - create a new view collection instead"
            )

          collection.system and new_name != old_name ->
            Repo.rollback("Cannot rename system collection '#{old_name}'")

          true ->
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
            if collection.type == "view" do
              Ecto.Adapters.SQL.query!(
                Repo,
                "DROP VIEW IF EXISTS #{TypeMapper.quote_ident(collection.name)} CASCADE",
                []
              )

              Lazypock.Realtime.Views.reset_view(collection.name)
            else
              Ecto.Adapters.SQL.query!(
                Repo,
                "DROP TABLE IF EXISTS #{TypeMapper.quote_ident(collection.name)} CASCADE",
                []
              )
            end

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
      system: Map.get(field_def, "system", false),
      sort_order: Map.get(field_def, "sort_order", 0)
    })
    |> Repo.insert!()
  end

  @doc """
  Resolves a relation field's target collection.

  The target may be given as a pre-set `options["collection"]` name or as a
  `collectionId` (top-level or inside `options`) holding a UUID or a name.
  Returns `{:ok, collection}` or `{:error, message}`. The error form lets
  callers fail fast on unresolvable targets (see `Lazypock.Migrations`)
  instead of silently persisting a broken relation.
  """
  @spec resolve_relation_target(map()) ::
          {:ok, Lazypock.Collections.Collection.t()} | {:error, String.t()}
  def resolve_relation_target(field_def) do
    name = field_def["name"] || ""
    opts = Map.get(field_def, "options", %{})

    cond do
      # Pre-set options.collection is a collection name — verify it exists.
      is_binary(opts["collection"]) and opts["collection"] != "" ->
        case get_collection(opts["collection"]) do
          {:ok, collection} ->
            {:ok, collection}

          {:error, :not_found} ->
            {:error,
             "Relation field \"#{name}\" references unknown collection \"#{opts["collection"]}\""}
        end

      true ->
        # Try to resolve collectionId (UUID from SPA, or collection name) to a
        # collection name. The id column is :binary_id, so only query by id when
        # the value is actually a UUID. Note: Ecto.UUID.cast/1 also hex-encodes
        # any 16-byte binary, so a bare name of exactly 16 characters must not
        # be mistaken for a UUID — gate on the canonical 36-char form and use
        # the cast *result* (the raw string would fail to dump to :binary_id).
        raw_id = field_def["collectionId"] || opts["collectionId"]

        if is_binary(raw_id) and raw_id != "" do
          collection =
            case Ecto.UUID.cast(raw_id) do
              {:ok, uuid} when byte_size(raw_id) == 36 ->
                Repo.get_by(Lazypock.Collections.Collection, id: uuid) ||
                  Repo.get_by(Lazypock.Collections.Collection, name: raw_id)

              _ ->
                Repo.get_by(Lazypock.Collections.Collection, name: raw_id)
            end

          case collection do
            nil ->
              {:error,
               "Relation field \"#{name}\" references unknown collection \"#{raw_id}\""}

            collection ->
              {:ok, collection}
          end
        else
          {:error,
           "Relation field \"#{name}\" requires a target collection (set options.collection or collectionId)"}
        end
    end
  end

  defp normalize_relation_opts(field_def) do
    opts = Map.get(field_def, "options", %{})

    # If options already has a "collection" key pointing to a name, use it
    if is_binary(opts["collection"]) and opts["collection"] != "" do
      opts
    else
      # Try to resolve collectionId (UUID from SPA, or collection name) to a
      # collection name. Failures are tolerated here on purpose — this path
      # serves the Studio, importer and backup restore, where a dangling
      # target (collection deleted after the field was created, out-of-order
      # import, reference to a collection outside the import set) must not
      # block the save.
      case resolve_relation_target(field_def) do
        {:ok, collection} -> Map.put(opts, "collection", collection.name)
        {:error, _msg} -> opts
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
