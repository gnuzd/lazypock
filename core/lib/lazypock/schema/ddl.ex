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
          #{TypeMapper.to_pg(field_def["type"])}
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
        if collection.managed do
          Ecto.Adapters.SQL.query!(
            Repo,
            "DROP TABLE IF EXISTS #{TypeMapper.quote_ident(collection.name)} CASCADE",
            []
          )

          Repo.delete!(collection)
          :ok
        else
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

    """
    CREATE TABLE #{TypeMapper.quote_ident(name)} (
      #{pk},
      #{Enum.join(columns, ",\n      ")},
      #{ts}
    )
    """
  end

  defp column_def(field) do
    col = TypeMapper.quote_ident(field["name"])
    pg_type = TypeMapper.to_pg(field["type"])
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

    %Lazypock.Collections.Collection{}
    |> Lazypock.Collections.Collection.changeset(%{
      name: name,
      type: type,
      schema: initial_schema,
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
