defmodule Lazypock.Schema.TypeMapper do
  @moduledoc """
  Maps LazyPock field types (e.g. "text", "bool", "email")
  to PostgreSQL column types and Ecto schema types.

  ## Supported types

    text, number, bool, email, url, date, select, multi_select,
    file, multi_file, json, relation, editor, password
  """

  @pg_types %{
    "text" => "TEXT",
    "number" => "NUMERIC",
    "bool" => "BOOLEAN",
    "email" => "TEXT",
    "url" => "TEXT",
    "date" => "TIMESTAMPTZ",
    "select" => "TEXT",
    "multi_select" => "TEXT[]",
    "file" => "TEXT",
    "multi_file" => "TEXT[]",
    "json" => "JSONB",
    "relation" => "UUID",
    "editor" => "TEXT",
    "password" => "TEXT"
  }

  @ecto_types %{
    "text" => :string,
    "number" => :decimal,
    "bool" => :boolean,
    "email" => :string,
    "url" => :string,
    "date" => :utc_datetime_usec,
    "select" => :string,
    "multi_select" => {:array, :string},
    "file" => :string,
    "multi_file" => {:array, :string},
    "json" => :map,
    "relation" => :binary_id,
    "editor" => :string,
    "password" => :string
  }

  @doc """
  Returns the PostgreSQL DDL column type for a given LazyPock field type.

  ## Examples

      iex> Lazypock.Schema.TypeMapper.to_pg("text")
      "TEXT"

      iex> Lazypock.Schema.TypeMapper.to_pg("bool")
      "BOOLEAN"

      iex> Lazypock.Schema.TypeMapper.to_pg("json")
      "JSONB"
  """
  @spec to_pg(String.t()) :: String.t()
  def to_pg(type) when is_binary(type) do
    Map.fetch!(@pg_types, type)
  end

  @doc """
  Returns the Ecto schema type for a given LazyPock field type.

  ## Examples

      iex> Lazypock.Schema.TypeMapper.to_ecto("text")
      :string

      iex> Lazypock.Schema.TypeMapper.to_ecto("bool")
      :boolean

      iex> Lazypock.Schema.TypeMapper.to_ecto("multi_select")
      {:array, :string}
  """
  @spec to_ecto(String.t()) :: atom() | tuple()
  def to_ecto(type) when is_binary(type) do
    Map.fetch!(@ecto_types, type)
  end

  @doc """
  Returns all valid field type strings.

  ## Examples

      iex> "text" in Lazypock.Schema.TypeMapper.valid_types()
      true

      iex> "invalid" in Lazypock.Schema.TypeMapper.valid_types()
      false
  """
  @spec valid_types() :: [String.t()]
  def valid_types do
    Map.keys(@pg_types)
  end

  @doc """
  Returns true if the given type string is valid.

  ## Examples

      iex> Lazypock.Schema.TypeMapper.valid_type?("text")
      true

      iex> Lazypock.Schema.TypeMapper.valid_type?("unknown")
      false
  """
  @spec valid_type?(String.t()) :: boolean()
  def valid_type?(type) when is_binary(type) do
    Map.has_key?(@pg_types, type)
  end

  @doc """
  Returns the SQL default value expression for a field definition.

  Returns empty string when there's no default or when the default is nil.
  """
  @spec default_sql(map()) :: String.t()
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

  @doc """
  Escapes a string for safe use in SQL DDL (single quotes only).
  """
  @spec escape_string(String.t()) :: String.t()
  def escape_string(value) when is_binary(value) do
    String.replace(value, "'", "''")
  end

  @doc """
  Quotes a PostgreSQL identifier (table name, column name).
  Uses double-quotes and escapes any existing double-quotes.
  """
  @spec quote_ident(String.t()) :: String.t()
  def quote_ident(name) when is_binary(name) do
    "\"#{String.replace(name, "\"", "\"\"")}\""
  end
end
