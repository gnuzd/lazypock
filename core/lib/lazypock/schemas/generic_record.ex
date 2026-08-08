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
  All return values are coerced to JSON-safe types.
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
      |> coerce_values_for_db()

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
        {:ok, row_to_map(cols, row)}

      {:error, err} ->
        {:error, Exception.message(err)}
    end
  end

  @doc """
  Returns all records from a collection as a list of maps.
  All values coerced to JSON-safe types.
  """
  @spec all(String.t()) :: [map()]
  def all(collection_name) when is_binary(collection_name) do
    sql = "SELECT * FROM #{TypeMapper.quote_ident(collection_name)}"

    case Ecto.Adapters.SQL.query(Repo, sql, []) do
      {:ok, result} -> result |> rows_to_maps() |> Enum.map(&coerce_row/1)
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
  def all_where(collection_name, where_clause \\ "", params \\ []) do
    sql = "SELECT * FROM #{TypeMapper.quote_ident(collection_name)}"

    sql =
      if has_where?(where_clause) do
        sql <> " WHERE " <> where_clause
      else
        sql <> " " <> where_clause
      end

    case Ecto.Adapters.SQL.query(Repo, sql, params) do
      {:ok, result} -> result |> rows_to_maps() |> Enum.map(&coerce_row/1)
      {:error, _} -> []
    end
  end

  @doc """
  Returns a single record by ID, or nil.
  """
  @spec get(String.t(), String.t()) :: map() | nil
  def get(collection_name, id) when is_binary(collection_name) do
    sql = "SELECT * FROM #{TypeMapper.quote_ident(collection_name)} WHERE id = $1"
    id_bin = maybe_uuid_to_bin(id)

    case Ecto.Adapters.SQL.query(Repo, sql, [id_bin]) do
      {:ok, %{rows: [row], columns: cols}} ->
        row_to_map(cols, row)

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
      |> coerce_values_for_db()

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
    WHERE id = $#{id_param}
    RETURNING *
    """

    id_bin = maybe_uuid_to_bin(id)
    values = Map.values(data) ++ [id_bin, DateTime.utc_now()]

    case Ecto.Adapters.SQL.query(Repo, sql, values) do
      {:ok, %{rows: [row], columns: cols}} ->
        row_to_map(cols, row)

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
    sql = "DELETE FROM #{TypeMapper.quote_ident(collection_name)} WHERE id = $1"
    id_bin = maybe_uuid_to_bin(id)

    case Ecto.Adapters.SQL.query(Repo, sql, [id_bin]) do
      {:ok, _} -> :ok
      {:error, err} -> {:error, Exception.message(err)}
    end
  end

  @doc """
  Returns the count of records matching a WHERE clause.
  """
  @spec count_where(String.t(), String.t(), [term()]) :: non_neg_integer()
  def count_where(collection_name, where_clause \\ "", params \\ []) do
    query = "SELECT COUNT(*) as cnt FROM #{TypeMapper.quote_ident(collection_name)}"

    query =
      if where_clause != "" and where_clause != nil,
        do: query <> " WHERE " <> where_clause,
        else: query

    case Ecto.Adapters.SQL.query(Repo, query, params) do
      {:ok, %{rows: [[count]]}} -> count
      {:error, _} -> 0
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

  # ── Private ──

  defp rows_to_maps(%{rows: rows, columns: cols}) do
    Enum.map(rows, fn row ->
      Enum.zip(cols, row) |> Map.new()
    end)
  end

  # Coerce a raw result row (cols + row data) into JSON-safe map.
  # Postgrex returns binary UUIDs, Decimal, Date, NaiveDateTime etc.
  # UUID binary coercion is applied only to columns that are actually UUID
  # (name is `id` or ends with `_id`/`_ref`) to avoid hex-encoding 16-char
  # text values (e.g. emails) stored in TEXT columns.
  defp row_to_map(cols, row) do
    cols
    |> Enum.zip(row)
    |> Enum.map(fn {col, val} ->
      {col, coerce_value(col, val)}
    end)
    |> Map.new()
  end

  defp coerce_row(map) do
    Map.new(map, fn {k, v} -> {k, coerce_value(k, v)} end)
  end

  defp coerce_value(col, v) when is_binary(v) and byte_size(v) == 16 do
    if uuid_column?(col) do
      Ecto.UUID.cast!(v)
    else
      v
    end
  end

  defp coerce_value(_col, %Decimal{} = d), do: Decimal.to_float(d)
  defp coerce_value(_col, %Date{} = d), do: Date.to_iso8601(d)
  defp coerce_value(_col, %NaiveDateTime{} = dt), do: NaiveDateTime.to_iso8601(dt)
  defp coerce_value(_col, %DateTime{} = dt), do: DateTime.to_iso8601(dt)
  defp coerce_value(_col, v), do: v

  defp uuid_column?(col) do
    col == "id" or String.ends_with?(col, "_id") or String.ends_with?(col, "_ref")
  end

  # Parse string date/datetime values into Elixir structs before sending to Postgrex.
  defp coerce_values_for_db(map) do
    Map.new(map, fn {key, value} ->
      {key, coerce_for_db(value)}
    end)
  end

  defp coerce_for_db(%DateTime{} = dt), do: dt
  defp coerce_for_db(%NaiveDateTime{} = dt), do: dt
  defp coerce_for_db(%Date{} = d), do: d

  defp coerce_for_db(value) when is_binary(value) do
    cond do
      # ISO 8601 datetime with timezone "2024-01-15T00:00:00Z" or "2024-01-15T00:00:00+00:00"
      String.match?(value, ~r/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}/) ->
        case DateTime.from_iso8601(value) do
          {:ok, dt, _} ->
            dt

          {:error, _} ->
            case NaiveDateTime.from_iso8601(value) do
              {:ok, ndt} -> ndt
              {:error, _} -> value
            end
        end

      # ISO 8601 date "2024-01-15"
      String.match?(value, ~r/^\d{4}-\d{2}-\d{2}$/) ->
        case Date.from_iso8601(value) do
          {:ok, d} -> d
          {:error, _} -> value
        end

      true ->
        value
    end
  end

  defp coerce_for_db(value), do: value

  # Detect if a clause starts with a SQL keyword vs being a WHERE condition
  defp has_where?(clause) do
    is_binary(clause) and clause != "" and
      not String.match?(clause, ~r/^\s*(ORDER BY|LIMIT|OFFSET|HAVING|GROUP BY)/i)
  end

  # Convert a UUID string to Postgrex-compatible binary for UUID columns
  # Convert a UUID string to Postgrex-compatible binary for UUID columns
  defp maybe_uuid_to_bin(id) when is_binary(id) do
    case Ecto.UUID.dump(id) do
      {:ok, bin} -> bin
      :error -> id
    end
  end

  defp maybe_uuid_to_bin(id), do: id
end
