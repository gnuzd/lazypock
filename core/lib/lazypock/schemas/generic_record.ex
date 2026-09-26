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

  # Postgres binds an Int16 parameter count, so a single statement can carry at
  # most 65 535 parameters. Batches are capped by `rows x columns + rows(id)`.
  @max_params 65_535
  @default_batch_size 500
  # A row bigger than this is inserted alone instead of being batched with others.
  @big_row_bytes 5_000_000

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
      |> put_autodate_now(collection_name, :on_create, now)
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

  `opts[:plain_id]` (used for view collections, whose `id` column is always
  text) passes the id through without the UUID->binary dump that regular
  uuid-backed tables need.
  """
  @spec get(String.t(), String.t()) :: map() | nil
  def get(collection_name, id) when is_binary(collection_name) do
    get(collection_name, id, [])
  end

  def get(collection_name, id, opts) when is_binary(collection_name) do
    sql = "SELECT * FROM #{TypeMapper.quote_ident(collection_name)} WHERE id = $1"
    id_bin = if Keyword.get(opts, :plain_id, false), do: id, else: maybe_uuid_to_bin(id)

    single_row_query(sql, [id_bin])
  end

  # Runs a query for a single-record operation, returning nil when the query
  # fails *or* when an argument cannot be encoded against its column type.
  #
  # A malformed record id from the URL (e.g. "not-a-uuid" against a uuid
  # column) reaches Postgrex as an un-encodable parameter, which raises
  # `DBConnection.EncodeError` *outside* the `{:error, _}` tuple path. That
  # used to surface as a 500; callers expect nil so they can answer 404 -- the
  # same fail-closed rule the rule enforcer applies to malformed record ids.
  defp single_row_query(sql, values) do
    case Ecto.Adapters.SQL.query(Repo, sql, values) do
      {:ok, %{rows: [row], columns: cols}} -> row_to_map(cols, row)
      {:ok, _} -> nil
      {:error, _} -> nil
    end
  rescue
    DBConnection.EncodeError -> nil
    ArgumentError -> nil
  end

  @doc """
  Updates a record by ID. Returns updated record as map or nil.
  """
  @spec update(String.t(), String.t(), map()) :: map() | nil
  def update(collection_name, id, attrs) when is_binary(collection_name) and is_map(attrs) do
    now = DateTime.utc_now()

    # `updated_at` is always bumped by this function (see below) — drop it
    # from the incoming attrs so it can never appear twice in the SET list.
    data =
      attrs
      |> Enum.map(fn {k, v} -> {to_string(k), v} end)
      |> Map.new()
      |> Map.drop(["updated_at"])
      |> put_autodate_now(collection_name, :on_update, now)
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
    values = Map.values(data) ++ [id_bin, now]

    single_row_query(sql, values)
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
  rescue
    # A malformed id cannot match any row, so there is nothing to delete.
    # Treat it as a no-op instead of crashing with EncodeError.
    DBConnection.EncodeError -> :ok
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

  @doc """
  Restores a record from a backup, preserving its identity.

  Unlike `insert/2`, this keeps the payload's `id` (when it is a valid
  UUID), `created_at` and `updated_at` — so relations between records stay
  intact across a backup restore. It is an **upsert**: a record with the
  same `id` already present is updated in place, making re-restores
  idempotent (no duplicate rows).

  When the payload has no `id` (or an id that isn't a valid UUID, e.g. a
  PocketBase-style id), a fresh UUID is generated — matching `insert/2`.
  Timestamps missing from the payload default to now.

  Returns `{:ok, record_map}` or `{:error, error_message}`.
  """
  @spec restore(String.t(), map()) :: {:ok, map()} | {:error, String.t()}
  def restore(collection_name, record) when is_binary(collection_name) and is_map(record) do
    %{id: id_bin, columns: columns, values: values} = prepare_record(collection_name, record)

    quoted = Enum.map(columns, &TypeMapper.quote_ident/1)

    set_clauses =
      quoted
      |> Enum.with_index(2)
      |> Enum.map(fn {col, idx} -> "#{col} = $#{idx}" end)
      |> Enum.join(", ")

    placeholders =
      case quoted do
        [] ->
          "($1)"

        _ ->
          "($1, " <> Enum.map_join(2..(length(quoted) + 1), ", ", &"$#{&1}") <> ")"
      end

    sql =
      insert_sql(collection_name, quoted, placeholders, set_clauses) <> " RETURNING *"

    case Ecto.Adapters.SQL.query(Repo, sql, [id_bin | values]) do
      {:ok, %{rows: [row], columns: cols}} ->
        {:ok, row_to_map(cols, row)}

      {:error, err} ->
        {:error, Exception.message(err)}
    end
  end

  @doc """
  Restores a stream/list of records with batched upserts (one statement per batch).

  Rows are grouped into batches that share the same column set (so a record that
  omits a column never nulls it out, matching `restore/2`), and a batch is capped
  by both row count (`:batch_size`, default 500) and the Postgres bind-parameter
  limit of 65 535 (`:max_params`). A single row larger than `:big_row_bytes`
  (default 5 MB) is flushed alone so one huge value cannot be dragged along with
  hundreds of small ones in the same oversized statement.

  Returns `{:ok, count}` or `{:error, reason}`; the first failing batch aborts.
  Accepts any enumerable (including a lazy `File.stream!/1`), so it never
  materializes the whole record set.
  """
  @spec restore_many(String.t(), Enumerable.t(), keyword()) ::
          {:ok, non_neg_integer()} | {:error, String.t()}
  def restore_many(collection_name, records, opts \\ []) when is_binary(collection_name) do
    max_params = Keyword.get(opts, :max_params, @max_params)
    max_rows = Keyword.get(opts, :batch_size, @default_batch_size)
    big_row_bytes = Keyword.get(opts, :big_row_bytes, @big_row_bytes)

    try do
      {count, state} =
        Enum.reduce(records, {0, nil}, fn record, {count, state} ->
          rec = prepare_record(collection_name, record, big_row_bytes)

          case state do
            nil ->
              {count, %{sig: rec.sig, batch: [rec], bytes: rec.size}}

            %{sig: sig, batch: batch, bytes: bytes} ->
              limit = div(max_params, max(length(rec.columns) + 1, 1))

              if sig == rec.sig and length(batch) < min(max_rows, limit) and
                   bytes + rec.size <= big_row_bytes do
                {count, %{sig: sig, batch: [rec | batch], bytes: bytes + rec.size}}
              else
                insert_batch!(collection_name, batch)
                {count + length(batch), %{sig: rec.sig, batch: [rec], bytes: rec.size}}
              end
          end
        end)

      if state, do: insert_batch!(collection_name, state.batch)

      {:ok, count + if(state, do: length(state.batch), else: 0)}
    rescue
      e -> {:error, Exception.message(e)}
    end
  end

  # One statement for a whole batch. `columns` come from the first row; every
  # row in the batch shares that column set by construction.
  defp insert_batch!(_collection_name, []), do: :ok

  defp insert_batch!(collection_name, batch) do
    columns = hd(batch).columns
    quoted = Enum.map(columns, &TypeMapper.quote_ident/1)
    width = length(columns) + 1

    params = Enum.flat_map(batch, fn rec -> [rec.id | rec.values] end)

    placeholders =
      batch
      |> Enum.with_index()
      |> Enum.map_join(", ", fn {_rec, row} ->
        base = row * width

        "(" <>
          Enum.map_join(1..width, ", ", fn i -> "$#{base + i}" end) <> ")"
      end)

    set_clauses =
      case quoted do
        [] -> nil
        _ -> Enum.map_join(quoted, ", ", fn col -> "#{col} = EXCLUDED.#{col}" end)
      end

    sql = insert_sql(collection_name, quoted, placeholders, set_clauses)

    Ecto.Adapters.SQL.query!(Repo, sql, params)
    :ok
  end

  defp insert_sql(collection_name, [], placeholders, _set) do
    # Record carrying only an id — nothing to update on conflict.
    "INSERT INTO #{TypeMapper.quote_ident(collection_name)} (\"id\") VALUES #{placeholders} ON CONFLICT (id) DO NOTHING"
  end

  defp insert_sql(collection_name, quoted, placeholders, set_clauses) do
    """
    INSERT INTO #{TypeMapper.quote_ident(collection_name)} ("id", #{Enum.join(quoted, ", ")})
    VALUES #{placeholders}
    ON CONFLICT (id) DO UPDATE SET
      #{set_clauses}
    """
  end

  @doc """
  Streams every record of a collection as coerced maps, using a server-side
  cursor (`Ecto.Adapters.SQL.stream/4` + Postgrex `DECLARE`/`FETCH`), so no more
  than `:max_rows` rows (default 500) are ever in memory.

  **Must be consumed inside a `Repo.transaction/2`** (Ecto requires it), and the
  transaction's isolation level applies to the snapshot it reads.
  """
  @spec stream_all(String.t(), keyword()) :: Enumerable.t()
  def stream_all(collection_name, opts \\ []) when is_binary(collection_name) do
    max_rows = Keyword.get(opts, :max_rows, @default_batch_size)
    sql = "SELECT * FROM #{TypeMapper.quote_ident(collection_name)}"

    Ecto.Adapters.SQL.stream(Repo, sql, [], max_rows: max_rows)
    |> Stream.flat_map(fn %{rows: rows, columns: cols} ->
      Enum.map(rows, &row_to_map(cols, &1))
    end)
  end

  @doc """
  Streams only the `id` of every record in a collection — useful when a caller
  needs ids without materializing full rows. Same transaction requirement as
  `stream_all/2`.
  """
  @spec stream_ids(String.t(), keyword()) :: Enumerable.t()
  def stream_ids(collection_name, opts \\ []) when is_binary(collection_name) do
    max_rows = Keyword.get(opts, :max_rows, @default_batch_size)
    sql = "SELECT id FROM #{TypeMapper.quote_ident(collection_name)}"

    Ecto.Adapters.SQL.stream(Repo, sql, [], max_rows: max_rows)
    |> Stream.flat_map(fn %{rows: rows} ->
      Enum.map(rows, fn [id] -> coerce_value("id", id) end)
    end)
  end

  # Normalizes one record into the exact shape the DB write needs.
  defp prepare_record(collection_name, record, big_row_bytes \\ @big_row_bytes) do
    data = normalize_record(collection_name, record)

    # `id` needs Postgrex's 16-byte binary encoding for the UUID column.
    id_bin = maybe_uuid_to_bin(Map.fetch!(data, "id"))

    # Build columns + values from ONE pass so their order always aligns.
    entries = Enum.reject(data, fn {k, _} -> k == "id" end)
    columns = Enum.map(entries, &elem(&1, 0))
    values = Enum.map(entries, &elem(&1, 1))

    %{
      id: id_bin,
      columns: columns,
      values: values,
      sig: columns,
      size: approximate_size(values, big_row_bytes)
    }
  end

  # Cheap size estimate used to decide whether a row must be inserted alone.
  # Deliberately does NOT re-encode the record: `Jason.encode!/1` on a 100 MB
  # value is exactly the cost this guard exists to avoid paying twice.
  defp approximate_size(values, cap) do
    Enum.reduce_while(values, 0, fn
      v, acc when is_binary(v) ->
        total = acc + byte_size(v)
        if total > cap, do: {:halt, total}, else: {:cont, total}

      _v, acc ->
        {:cont, acc + 8}
    end)
  end

  defp normalize_record(collection_name, record) do
    now = DateTime.utc_now()

    record
    |> Enum.map(fn {k, v} -> {to_string(k), v} end)
    |> Map.new()
    |> coerce_values_for_db()
    |> restore_ensure_id(now)
    |> restore_ensure_timestamps(now)
    |> restore_default_arrays(collection_name)
  end

  # ── Restore helpers ────────────────────────────────

  defp restore_ensure_id(data, _now) do
    # Missing id → generate one (matches insert/2 auto-gen). Invalid UUID
    # (e.g. PocketBase-style id) → same, let the DB generate it.
    data = Map.put_new(data, "id", Ecto.UUID.generate())

    case Map.get(data, "id") do
      id when is_binary(id) ->
        case Ecto.UUID.cast(id) do
          {:ok, _} -> data
          :error -> Map.put(data, "id", Ecto.UUID.generate())
        end

      _ ->
        Map.put_new(data, "id", Ecto.UUID.generate())
    end
  end

  defp restore_ensure_timestamps(data, now) do
    data
    |> Map.put_new("created_at", now)
    |> Map.put_new("updated_at", now)
  end

  defp restore_default_arrays(data, collection_name) do
    fields = collection_fields(collection_name) || []

    Enum.reduce(fields, data, fn field, acc ->
      name = to_string(field.name)
      pg_type = Lazypock.Schema.TypeMapper.column_pg_type(field)

      cond do
        Map.has_key?(acc, name) ->
          acc

        pg_type == "TEXT[]" ->
          Map.put(acc, name, [])

        true ->
          acc
      end
    end)
  end

  # ── Autodate fields ─────────────────────────────────

  # Autodate fields (PocketBase-style, type `autodate` with an
  # `onCreate`/`onUpdate` option) are maintained by the write path:
  # onCreate fields are stamped with `now` on insert, onUpdate fields on
  # update. Values are looked up from the collection's field metadata via
  # the Registry (no-op when the registry isn't loaded, e.g. before the
  # app has booted — at that point no record writes happen).
  #
  # The system `created_at`/`updated_at` columns are excluded: insert/2
  # stamps them explicitly and update/3 always bumps `updated_at` in its
  # own SET clause — including them again would emit the column twice.
  defp put_autodate_now(data, collection_name, trigger, now) do
    Enum.reduce(autodate_columns(collection_name, trigger), data, fn col, acc ->
      Map.put(acc, col, now)
    end)
  end

  defp autodate_columns(collection_name, trigger) do
    (collection_fields(collection_name) || [])
    |> Enum.flat_map(fn field ->
      name = to_string(field.name)

      if name not in TypeMapper.system_timestamp_columns() and
           TypeMapper.autodate_trigger?(field, trigger) do
        [name]
      else
        []
      end
    end)
  end

  defp collection_fields(collection_name) do
    case Lazypock.Collections.Registry.get(collection_name) do
      {:ok, collection} -> collection.fields
      _ -> nil
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
  # UUID columns get Postgrex's 16-byte binary encoding; integer ids
  # (bigint/serial tables from raw `create table` migrations) are passed as
  # integers so Postgrex encodes them against the numeric column instead of
  # raising ("expected an integer, got \"1\""); anything else passes through.
  defp maybe_uuid_to_bin(id) when is_binary(id) do
    case Ecto.UUID.dump(id) do
      {:ok, bin} -> bin
      :error -> maybe_integer_id(id)
    end
  end

  defp maybe_uuid_to_bin(id), do: id

  defp maybe_integer_id(id) do
    case Integer.parse(id) do
      {int, ""} -> int
      _ -> id
    end
  end
end
