defmodule Lazypock.Schema.DDL do
  @moduledoc """
  Executes DDL operations safely using PostgreSQL's transactional DDL.

  All DDL operations run inside `Repo.transaction` — if anything fails,
  the entire transaction (including DDL) rolls back automatically.
  PubSub broadcasts happen AFTER the commit so the Registry
  (which connects via a different DB connection) sees committed data.
  """

  alias Lazypock.Repo
  alias Lazypock.Schema.TypeMapper
  import Ecto.Query

  # ── Create collection ──────────────────────────────

  @doc """
  Creates a new collection table and stores its metadata.

  ## Options

    * `:type` - Collection type: `"base"` (default) or `"auth"`
    * `:fields` - List of field definition maps (see README for format)
  """
  @spec create_collection(String.t(), keyword()) ::
          {:ok, Lazypock.Collections.Collection.t()} | {:error, term()}
  def create_collection(name, opts \\ []) when is_binary(name) do
    type = Keyword.get(opts, :type, "base")
    fields = Keyword.get(opts, :fields, [])

    result =
      Repo.transaction(fn ->
        with :ok <- validate_collection_name(name),
             :ok <- validate_collection_not_exists(name),
             :ok <- validate_fields(fields) do
          lock_key = :erlang.phash2({:create_collection, name})
          Ecto.Adapters.SQL.query!(Repo, "SELECT pg_advisory_xact_lock(#{lock_key})", [])

          collection = create_collection_metadata!(name, type, fields)

          sql = build_create_table_sql(name, fields)
          Ecto.Adapters.SQL.query!(Repo, sql, [])

          create_indexes(name, fields)
          create_field_metadata!(collection.id, fields)

          Repo.preload(collection, :fields)
        end
      end)

    case result do
      {:ok, collection} ->
        Phoenix.PubSub.broadcast(Lazypock.PubSub, "schema", {:collection_created, collection})
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
          ADD COLUMN #{TypeMapper.quote_ident(field_def["name"])}
          #{TypeMapper.to_pg_with_opts(field_def["type"], field_def["options"] || %{})}
          #{if field_def["required"], do: " NOT NULL", else: ""}
          #{TypeMapper.default_sql(field_def)}
          """

          Ecto.Adapters.SQL.query!(Repo, sql, [])

          if field_def["indexed"] do
            create_index(collection_name, field_def["name"])
          end

          create_field_metadata_entry!(collection.id, field_def)
          update_collection_schema!(collection)

          :ok
        end
      end)

    case result do
      {:ok, :ok} ->
        Phoenix.PubSub.broadcast(
          Lazypock.PubSub,
          "schema",
          {:field_added, collection_name, field_def}
        )

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
        Phoenix.PubSub.broadcast(
          Lazypock.PubSub,
          "schema",
          {:field_removed, collection_name, field_name}
        )

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
  """
  @spec update_collection(String.t(), keyword()) :: {:ok, map()} | {:error, term()}
  def update_collection(old_name, opts \\ []) when is_binary(old_name) do
    new_name = Keyword.get(opts, :name, old_name)
    type = Keyword.get(opts, :type)
    fields = Keyword.get(opts, :fields, [])

    result =
      Repo.transaction(fn ->
        {:ok, collection} = get_collection(old_name)

        # System collections cannot be renamed
        if collection.system and new_name != old_name do
          {:error, "Cannot rename system collection '#{old_name}'"}
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

          existing_fields =
            Repo.all(
              from(f in Lazypock.Collections.Field, where: f.collection_id == ^collection.id)
            )

          existing_names = MapSet.new(existing_fields, & &1.name)
          incoming_names = MapSet.new(fields, & &1["name"])

          # Remove deleted fields
          for f <- existing_fields, not MapSet.member?(incoming_names, f.name) do
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
            fields
            |> Enum.with_index()
            |> Enum.map(fn {f, idx} ->
              Map.put(f, "sort_order", idx)
            end)

          for f <- fields_with_sort, MapSet.member?(existing_names, f["name"]) do
            Repo.update_all(
              from(fld in Lazypock.Collections.Field,
                where: fld.collection_id == ^collection.id and fld.name == ^f["name"]
              ),
              set: [
                required: Map.get(f, "required", false),
                unique: Map.get(f, "unique", false),
                hidden: Map.get(f, "hidden", false),
                system: Map.get(f, "system", false),
                options: Map.get(f, "options", %{}),
                indexed: Map.get(f, "indexed", false),
                sort_order: f["sort_order"]
              ]
            )
          end

          # Add new fields (also derive sort_order from position)
          for f <- fields_with_sort, not MapSet.member?(existing_names, f["name"]) do
            :ok = validate_field_name!(f["name"])
            :ok = validate_field_type(f["type"])

            sql =
              "ALTER TABLE #{TypeMapper.quote_ident(new_name)} ADD COLUMN #{TypeMapper.quote_ident(f["name"])} #{TypeMapper.to_pg_with_opts(f["type"], f["options"] || %{})} #{if f["required"], do: " NOT NULL", else: ""} #{TypeMapper.default_sql(f)}"

            Ecto.Adapters.SQL.query!(Repo, sql, [])
            if f["indexed"], do: create_index(new_name, f["name"])
            create_field_metadata_entry!(collection.id, f)
          end

          collection_refresh =
            Repo.get!(Lazypock.Collections.Collection, collection.id) |> Repo.preload(:fields)

          update_collection_schema!(collection_refresh)
          collection_refresh
        end
      end)

    case result do
      {:ok, collection} ->
        Phoenix.PubSub.broadcast(Lazypock.PubSub, "schema", {:collection_updated, collection})
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
        Phoenix.PubSub.broadcast(
          Lazypock.PubSub,
          "schema",
          {:collection_deleted, collection.name}
        )

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
      Enum.any?(names, &(not (&1 =~ ~r/^[a-z][a-z0-9_]*$/))) ->
        {:error,
         "Field names must start with a letter and contain only lowercase letters, numbers, and underscores"}

      length(Enum.uniq(names)) != length(names) ->
        {:error, "Duplicate field names are not allowed"}

      Enum.any?(fields, &(not TypeMapper.valid_type?(&1["type"]))) ->
        {:error, "Invalid field type"}

      true ->
        :ok
    end
  end

  defp validate_field_name!(field_name) do
    if field_name =~ ~r/^[a-z][a-z0-9_]*$/ do
      :ok
    else
      {:error,
       "Field name must start with a letter and contain only lowercase letters, numbers, and underscores"}
    end
  end

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
    col = TypeMapper.quote_ident(field["name"])
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
    index_name = "#{table}_#{column}_idx"

    Ecto.Adapters.SQL.query!(
      Repo,
      "CREATE INDEX IF NOT EXISTS #{TypeMapper.quote_ident(index_name)} ON #{TypeMapper.quote_ident(table)} (#{TypeMapper.quote_ident(column)})",
      []
    )
  end

  defp create_unique_index(table, column) do
    index_name = "#{table}_#{column}_unq"

    Ecto.Adapters.SQL.query!(
      Repo,
      "CREATE UNIQUE INDEX IF NOT EXISTS #{TypeMapper.quote_ident(index_name)} ON #{TypeMapper.quote_ident(table)} (#{TypeMapper.quote_ident(column)})",
      []
    )
  end

  defp create_collection_metadata!(name, type, fields) do
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

    %Lazypock.Collections.Collection{}
    |> Lazypock.Collections.Collection.changeset(%{
      name: name,
      type: type,
      schema: initial_schema,
      rules: default_rules,
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
    %Lazypock.Collections.Field{}
    |> Lazypock.Collections.Field.changeset(%{
      collection_id: collection_id,
      name: field_def["name"],
      type: field_def["type"],
      required: Map.get(field_def, "required", false),
      unique: Map.get(field_def, "unique", false),
      default_value: field_def["default"],
      options: Map.get(field_def, "options", %{}),
      indexed: Map.get(field_def, "indexed", false),
      sort_order: Map.get(field_def, "sort_order", 0)
    })
    |> Repo.insert!()
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
