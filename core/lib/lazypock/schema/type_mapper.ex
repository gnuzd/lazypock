defmodule Lazypock.Schema.TypeMapper do
  @moduledoc """
  Maps LazyPock field types to PostgreSQL column types.
  """
  @pg_types %{
    "text" => "TEXT",
    "number" => "NUMERIC",
    "bool" => "BOOLEAN",
    "email" => "TEXT",
    "url" => "TEXT",
    "date" => "TIMESTAMPTZ",
    "datetime" => "TIMESTAMPTZ",
    "select" => "TEXT",
    "multi_select" => "TEXT[]",
    "file" => "TEXT",
    "multi_file" => "TEXT[]",
    "json" => "JSONB",
    "relation" => "TEXT",
    "editor" => "TEXT",
    "password" => "TEXT",
    "geo" => "JSONB"
  }

  @doc """
  Returns the PostgreSQL DDL type, considering options like maxSelect.
  Multi-relations get TEXT[] instead of TEXT.
  """
  def to_pg_with_opts("relation", %{"maxSelect" => max}) when is_integer(max) and max > 1 do
    "TEXT[]"
  end

  def to_pg_with_opts(type, _opts), do: Map.fetch!(@pg_types, type)

  @doc """
  PostgreSQL type of the system `id` column every collection gets
  (see `DDL.build_create_table_sql/2`): `id UUID PRIMARY KEY ...`.
  """
  @spec id_column_type() :: String.t()
  def id_column_type, do: "UUID"

  @doc """
  Returns the PostgreSQL type of a field definition for use in casts.

  Accepts either a raw field map (`%{"type" => ...}`) or a
  `Lazypock.Collections.Field` struct.
  """
  @spec column_pg_type(map()) :: String.t()
  def column_pg_type(field) when is_map(field) do
    type = Map.get(field, "type") || Map.get(field, :type)
    opts = Map.get(field, "options") || Map.get(field, :options) || %{}
    to_pg_with_opts(type, opts)
  end

  @doc """
  Coerces a runtime value to the Elixir representation Postgrex needs in order
  to encode it as a bound parameter of the given PostgreSQL type.

  Returns `{:ok, coerced}` or `:error` when the value cannot be represented
  (e.g. a non-UUID string against a `uuid` column, or an empty string against
  one). Callers treat `:error` as "no match" — matching the previous behavior
  where the equivalent inline literal produced a PostgreSQL error that was
  mapped to a denial.
  """
  @spec coerce_value(String.t(), term()) :: {:ok, term()} | :error
  def coerce_value(_pg_type, nil), do: {:ok, nil}

  def coerce_value("TEXT", value) when is_binary(value), do: {:ok, value}
  def coerce_value("TEXT", _value), do: :error

  def coerce_value("UUID", value) when is_binary(value) and byte_size(value) == 16,
    do: {:ok, value}

  def coerce_value("UUID", value) when is_binary(value) do
    case Ecto.UUID.dump(value) do
      {:ok, bin} -> {:ok, bin}
      :error -> :error
    end
  end

  def coerce_value("NUMERIC", %Decimal{} = value), do: {:ok, value}
  def coerce_value("NUMERIC", value) when is_integer(value) or is_float(value), do: {:ok, value}

  def coerce_value("NUMERIC", value) when is_binary(value) do
    case Decimal.parse(value) do
      {decimal, ""} -> {:ok, decimal}
      _ -> :error
    end
  end

  def coerce_value("BOOLEAN", value) when is_boolean(value), do: {:ok, value}

  def coerce_value("BOOLEAN", value) when value in ["true", "false"],
    do: {:ok, value == "true"}

  def coerce_value("BOOLEAN", _value), do: :error

  def coerce_value("TIMESTAMPTZ", %DateTime{} = value), do: {:ok, value}
  def coerce_value("TIMESTAMPTZ", %NaiveDateTime{} = value), do: {:ok, value}

  def coerce_value("TIMESTAMPTZ", value) when is_binary(value) do
    case DateTime.from_iso8601(value) do
      {:ok, datetime, _offset} ->
        {:ok, datetime}

      _ ->
        # Date-only literals (e.g. '2024-01-01') cast to midnight UTC in PG.
        case Date.from_iso8601(value) do
          {:ok, date} ->
            case DateTime.new(date, ~T[00:00:00], "Etc/UTC") do
              {:ok, datetime} -> {:ok, datetime}
              _ -> :error
            end

          _ ->
            :error
        end
    end
  end

  def coerce_value("JSONB", value) when is_binary(value) do
    case Jason.decode(value) do
      {:ok, term} -> {:ok, term}
      _ -> :error
    end
  end

  def coerce_value("JSONB", value), do: {:ok, value}
  def coerce_value(_pg_type, _value), do: :error

  @doc """
  Returns all valid field type strings.
  """
  def valid_types, do: Map.keys(@pg_types)

  @doc """
  Returns true if the given type string is valid.
  """
  def valid_type?(type) when is_binary(type), do: Map.has_key?(@pg_types, type)

  @doc """
  Returns the SQL default value expression for a field definition.
  """
  def default_sql(%{"default" => nil}), do: ""

  def default_sql(%{"default" => default, "type" => type}) do
    case type do
      "text" -> "DEFAULT '#{escape_string(default)}'"
      "number" -> "DEFAULT #{default}"
      "bool" -> "DEFAULT #{if default, do: "TRUE", else: "FALSE"}"
      "date" -> "DEFAULT '#{default}'"
      _ -> ""
    end
  end

  def default_sql(_), do: ""

  @spec escape_string(String.t()) :: String.t()
  def escape_string(value) when is_binary(value), do: String.replace(value, "'", "''")

  @spec quote_ident(String.t()) :: String.t()
  def quote_ident(name) when is_binary(name), do: "\"#{String.replace(name, "\"", "\"\"")}\""
end
