defmodule Lazypock.Schemas.FilterCompiler do
  @moduledoc """
  Compiles PocketBase-compatible filter syntax into safe SQL WHERE clauses
  with parameterized values.
  """

  # Operators ordered longest-first to avoid prefix matching

  @doc """
  Compiles a PocketBase filter string into a SQL WHERE clause with parameters.
  """
  @spec compile(String.t()) :: {:ok, {String.t(), [term()]}} | {:error, String.t()}
  def compile(filter_str) when is_binary(filter_str) do
    filter_str = String.trim(filter_str)

    if filter_str == "" do
      {:ok, {"", []}}
    else
      {:ok, tokens} = tokenize(filter_str)

      case parse_or(tokens) do
        {:ok, ast, []} ->
          {sql, params} = emit(ast, 1)
          {:ok, {sql, params}}

        {:ok, _ast, leftover} ->
          {:error, "Unexpected tokens after expression: #{inspect(leftover)}"}

        :error ->
          {:error, "Failed to parse filter expression"}
      end
    end
  end

  @doc """
  Applies a compiled filter to a base SQL query string, adding WHERE clause.
  """
  @spec apply(String.t(), String.t(), [term()]) :: {String.t(), [term()]}
  def apply(base_sql, filter_clause, base_params \\ []) do
    case compile(filter_clause) do
      {:ok, {"", _}} -> {base_sql, base_params}
      {:ok, {where_sql, params}} -> {base_sql <> " WHERE " <> where_sql, base_params ++ params}
      {:error, _} -> {base_sql, base_params}
    end
  end

  # ── Tokenizer ────────────────────────────────────────

  defp tokenize(str) do
    # Use regex to split on operators with lookarounds to handle adjacent chars
    # Split on any of: && || >= <= != !~ > < ~ = ! ( )
    tokens =
      Regex.split(
        ~r/(&&|\|\||>=|<=|!=|!~|>|<|~|=|!|[()])/,
        str,
        include_captures: true,
        trim: true
      )
      |> Enum.map(&String.trim/1)
      |> Enum.reject(&(&1 == ""))

    {:ok, tokens}
  end

  # ── Parser ───────────────────────────────────────────

  defp parse_or(tokens) do
    case parse_and(tokens) do
      {:ok, left, ["||" | rest]} ->
        case parse_or(rest) do
          {:ok, right, remaining} -> {:ok, {:or, left, right}, remaining}
          _ -> {:ok, left, rest}
        end

      {:ok, ast, rest} -> {:ok, ast, rest}
      error -> error
    end
  end

  defp parse_and(tokens) do
    case parse_not(tokens) do
      {:ok, left, ["&&" | rest]} ->
        case parse_and(rest) do
          {:ok, right, remaining} -> {:ok, {:and, left, right}, remaining}
          _ -> {:ok, left, rest}
        end

      {:ok, ast, rest} -> {:ok, ast, rest}
      error -> error
    end
  end

  defp parse_not(["!" | rest]) do
    case parse_primary(rest) do
      {:ok, expr, remaining} -> {:ok, {:not, expr}, remaining}
      _ -> :error
    end
  end

  defp parse_not(tokens), do: parse_comparison(tokens)

  defp parse_comparison(tokens) do
    case parse_primary(tokens) do
      {:ok, left, [op | value_tokens]} when op in ~w(= != ~ !~ > >= < <=) ->
        case value_tokens do
          [token | rest] ->
            value = classify_literal(token)
            {:ok, {op, left, {:literal, value}}, rest}

          [] ->
            :error
        end

      {:ok, ast, rest} -> {:ok, ast, rest}
      error -> error
    end
  end

  defp parse_primary(["(" | rest]) do
    case parse_or(rest) do
      {:ok, expr, [")" | remaining]} -> {:ok, expr, remaining}
      _ -> :error
    end
  end

  defp parse_primary([token | rest]) do
    case classify(token) do
      {:field, name} -> {:ok, {:field, name}, rest}
      {:literal, value} -> {:ok, {:literal, value}, rest}
      _ -> :error
    end
  end

  defp parse_primary([]), do: :error

  defp classify(token) do
    cond do
      token in ~w(true True TRUE) -> {:literal, true}
      token in ~w(false False FALSE) -> {:literal, false}
      token in ~w(null Null NULL) -> {:literal, nil}
      String.starts_with?(token, "'") and String.ends_with?(token, "'") ->
        {:literal, String.slice(token, 1, String.length(token) - 2)}
      String.match?(token, ~r/^\d+(\.\d+)?$/) ->
        val = if String.contains?(token, "."), do: Decimal.new(token), else: String.to_integer(token)
        {:literal, val}
      String.match?(token, ~r/^[a-zA-Z_][a-zA-Z0-9_@]*$/) ->
        {:field, token}
      true ->
        :error
    end
  end

  defp classify_literal(token) do
    cond do
      token in ~w(true True TRUE) -> true
      token in ~w(false False FALSE) -> false
      token in ~w(null Null NULL) -> nil
      String.starts_with?(token, "'") and String.ends_with?(token, "'") ->
        String.slice(token, 1, String.length(token) - 2)
      String.match?(token, ~r/^\d+(\.\d+)?$/) ->
        if String.contains?(token, "."), do: Decimal.new(token), else: String.to_integer(token)
      true ->
        token
    end
  end

  # ── Code generator ───────────────────────────────────

  defp emit({:or, left, right}, idx) do
    {l_sql, l_params, idx2} = emit_one(left, idx)
    {r_sql, r_params, _idx3} = emit_one(right, idx2)
    {"(#{l_sql} OR #{r_sql})", l_params ++ r_params}
  end

  defp emit({:and, left, right}, idx) do
    {l_sql, l_params, idx2} = emit_one(left, idx)
    {r_sql, r_params, _idx3} = emit_one(right, idx2)
    {"(#{l_sql} AND #{r_sql})", l_params ++ r_params}
  end

  defp emit({:not, expr}, idx) do
    {sql, params, _} = emit_one(expr, idx)
    {"NOT #{sql}", params}
  end

  defp emit(op_ast, idx) do
    {sql, params} = emit_simple(op_ast)
    {renumber(sql, idx), params}
  end

  defp emit_simple({"~", {:field, f}, {:literal, val}}) do
    {~s["#{f}" ILIKE $1], ["%" <> val <> "%"]}
  end

  defp emit_simple({"!~", {:field, f}, {:literal, val}}) do
    {~s["#{f}" NOT ILIKE $1], ["%" <> val <> "%"]}
  end

  defp emit_simple({op, {:field, f}, {:literal, val}}) when op in ~w(= != > >= < <=) do
    {~s["#{f}" #{op} $1], [val]}
  end

  defp emit_simple({:field, name}) do
    {~s["#{name}"], []}
  end

  defp emit_one(ast, idx) do
    {sql, params} = emit(ast, idx)
    {sql, params, idx + length(params)}
  end

  defp renumber(sql, 1), do: sql
  defp renumber(sql, n), do: String.replace(sql, "$1", "$#{n}")
end
