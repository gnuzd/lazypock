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
