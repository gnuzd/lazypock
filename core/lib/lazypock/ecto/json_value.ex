defmodule Lazypock.Ecto.JSONValue do
  @moduledoc """
  A custom Ecto type that stores any JSON-compatible value as TEXT
  but round-trips through JSON encoding/decoding.

  This allows storing booleans, numbers, strings, arrays, and maps
  in a single TEXT column while preserving their original types.

  ## Examples

      iex> Lazypock.Ecto.JSONValue.load("true")
      {:ok, true}

      iex> Lazypock.Ecto.JSONValue.load("42")
      {:ok, 42}

      iex> Lazypock.Ecto.JSONValue.load("\"hello\"")
      {:ok, "hello"}

      iex> Lazypock.Ecto.JSONValue.load(nil)
      {:ok, nil}

      iex> Lazypock.Ecto.JSONValue.dump(false)
      {:ok, "false"}
  """

  use Ecto.Type

  def type, do: :string

  def cast(value) do
    {:ok, Jason.encode!(value)}
  end

  def load(nil), do: {:ok, nil}

  def load(value) when is_binary(value) do
    case Jason.decode(value) do
      {:ok, decoded} -> {:ok, decoded}
      _ -> {:ok, value}
    end
  end

  def load(value), do: {:ok, value}

  def dump(nil), do: {:ok, nil}

  def dump(value) when is_binary(value) do
    {:ok, value}
  end

  def dump(value) do
    {:ok, Jason.encode!(value)}
  end
end
