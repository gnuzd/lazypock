defmodule Lazypock.Schemas.GenericRecord do
  @moduledoc """
  SQL-based helpers for dynamic (user-created) collection tables.

  Since user tables have columns unknown at compile time, we use raw SQL
  for all operations instead of Ecto schemas. This is more transparent
  and avoids fighting Ecto's compile-time struct assumptions.

  ## Usage

      Lazypock.Schemas.GenericRecord.insert("posts", %{title: "Hello"})
      Lazypock.Schemas.GenericRecord.all("posts")
      Lazypock.Schemas.GenericRecord.get("posts", "abc-123")
  """

  alias Lazypock.Repo
  alias Lazypock.Schema.TypeMapper

  @doc """
  Inserts a record into a dynamic collection table.

  Returns `{:ok, record_map}` or `{:error, error_message}`.
  """
  @spec insert(String.t(), map()) :: {:ok, map()} | {:error, String.t()}
  def insert(collection_name, attrs) when is_binary(collection_name) and is_map(attrs) do
    now = DateTime.utc_now()

    data =
      attrs
      |> Enum.map(fn {k, v} -> {to_string(k), v} end)
      |> Map.new()
      |> Map.put("created_at", now)
      |> Map.put("updated_at", now)

    columns = Map.keys(data) |> Enum.map(&TypeMapper.quote_ident/1) |> Enum.join(", ")
    placeholders = 1..map_size(data) |> Enum.map(&"$#{&1}") |> Enum.join(", ")
    values = Map.values(data)

    sql = """
    INSERT INTO #{TypeMapper.quote_ident(collection_name)} (#{columns})
    VALUES (#{placeholders})
    RETURNING *
    """

    case Ecto.Adapters.SQL.query(Repo, sql, values) do
      {:ok, %{rows: [row], columns: cols}} ->
        {:ok, Enum.zip(cols, row) |> Map.new()}

      {:error, err} ->
        {:error, Exception.message(err)}
    end
  end

  @doc """
  Returns all records from a collection as a list of maps.
  """
  @spec all(String.t()) :: [map()]
  def all(collection_name) when is_binary(collection_name) do
    sql = "SELECT * FROM #{TypeMapper.quote_ident(collection_name)}"

    case Ecto.Adapters.SQL.query(Repo, sql, []) do
      {:ok, result} -> rows_to_maps(result)
      {:error, _} -> []
    end
  end

  @doc """
  Returns records matching a WHERE clause.
  Use `$1`, `$2` etc. as placeholders.

  ## Examples

      GenericRecord.all_where("posts", "title = $1", ["Hello"])
  """
  @spec all_where(String.t(), String.t(), [term()]) :: [map()]
  def all_where(collection_name, where_clause, params \\ []) do
    sql = "SELECT * FROM #{TypeMapper.quote_ident(collection_name)} WHERE #{where_clause}"

    case Ecto.Adapters.SQL.query(Repo, sql, params) do
      {:ok, result} -> rows_to_maps(result)
      {:error, _} -> []
    end
  end

  @doc """
  Returns a single record by ID, or nil.
  """
  @spec get(String.t(), String.t()) :: map() | nil
  def get(collection_name, id) when is_binary(collection_name) do
    sql = "SELECT * FROM #{TypeMapper.quote_ident(collection_name)} WHERE id = $1::uuid"

    case Ecto.Adapters.SQL.query(Repo, sql, [id]) do
      {:ok, %{rows: [row], columns: cols}} ->
        Enum.zip(cols, row) |> Map.new()

      {:ok, _} ->
        nil

      {:error, _} ->
        nil
    end
  end

  @doc """
  Updates a record by ID. Returns updated record as map or nil.
  """
  @spec update(String.t(), String.t(), map()) :: map() | nil
  def update(collection_name, id, attrs) when is_binary(collection_name) and is_map(attrs) do
    data =
      attrs
      |> Enum.map(fn {k, v} -> {to_string(k), v} end)
      |> Map.new()

    set_clauses =
      data
      |> Enum.with_index(1)
      |> Enum.map(fn {{col, _val}, idx} ->
        "#{TypeMapper.quote_ident(col)} = $#{idx}"
      end)
      |> Enum.join(", ")

    id_param = map_size(data) + 1

    sql = """
    UPDATE #{TypeMapper.quote_ident(collection_name)}
    SET #{set_clauses}, "updated_at" = $#{id_param + 1}
    WHERE id = $#{id_param}::uuid
    RETURNING *
    """

    values = Map.values(data) ++ [id, DateTime.utc_now()]

    case Ecto.Adapters.SQL.query(Repo, sql, values) do
      {:ok, %{rows: [row], columns: cols}} ->
        Enum.zip(cols, row) |> Map.new()

      {:ok, _} ->
        nil

      {:error, _} ->
        nil
    end
  end

  @doc """
  Deletes a record by ID. Returns `:ok` or `{:error, reason}`.
  """
  @spec delete(String.t(), String.t()) :: :ok | {:error, String.t()}
  def delete(collection_name, id) when is_binary(collection_name) do
    sql = "DELETE FROM #{TypeMapper.quote_ident(collection_name)} WHERE id = $1::uuid"

    case Ecto.Adapters.SQL.query(Repo, sql, [id]) do
      {:ok, _} -> :ok
      {:error, err} -> {:error, Exception.message(err)}
    end
  end

  @doc """
  Returns the count of records in a collection.
  """
  @spec count(String.t()) :: non_neg_integer()
  def count(collection_name) when is_binary(collection_name) do
    sql = "SELECT COUNT(*) as cnt FROM #{TypeMapper.quote_ident(collection_name)}"

    case Ecto.Adapters.SQL.query(Repo, sql, []) do
      {:ok, %{rows: [[count]]}} -> count
      {:error, _} -> 0
    end
  end

  # ── Private ──

  defp rows_to_maps(%{rows: rows, columns: cols}) do
    Enum.map(rows, fn row ->
      Enum.zip(cols, row) |> Map.new()
    end)
  end
end
