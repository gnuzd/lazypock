defmodule Lazypock.Schemas.FilterCompiler do
  @moduledoc """
  Compiles PocketBase-compatible filter syntax into safe SQL WHERE clauses
  with parameterized values.
  """

  alias Lazypock.Schema.TypeMapper

  @doc """
  Compiles a PocketBase filter string into a SQL WHERE clause with parameters.

  ## Bound token values and column casts

  When a filter string already contains `$N` placeholders (produced by the
  enforcer's token resolution, e.g. `@request.auth.id` → `$1`), pass the
  matching `token_values` list so the emitted clause binds those values as
  parameters instead of re-parsing them. When a `types` map (column name →
  PostgreSQL type) is provided, placeholders compared against typed columns
  get an explicit cast (e.g. `\"id\" = $1::UUID`) and their values are coerced
  to the representation Postgrex needs — this is what lets the enforcer bind
  values against `uuid`/`numeric`/etc. columns instead of inlining literals.
  """
  @spec compile(String.t()) :: {:ok, {String.t(), [term()]}} | {:error, String.t()}
  @spec compile(String.t(), [term()], map()) ::
          {:ok, {String.t(), [term()]}} | {:error, String.t()}
  def compile(filter_str, token_values \\ [], types \\ %{})
      when is_binary(filter_str) and is_list(token_values) and is_map(types) do
    filter_str = String.trim(filter_str)

    if filter_str == "" do
      {:ok, {"", []}}
    else
      {:ok, tokens} = tokenize(filter_str)

      case parse_or(tokens) do
        {:ok, ast, []} ->
          case coerce_ast(ast, types, token_values) do
            {:ok, coerced_ast} ->
              {sql, params} = emit(coerced_ast, 1, {types, token_values})
              {:ok, {sql, params}}

            :error ->
              {:error, "Rule value cannot be represented as its column type"}
          end

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

  @doc """
  Shifts every `$N` placeholder in a compiled clause by `offset`, so the clause
  can be embedded into a larger query whose earlier placeholders are already
  bound (e.g. the enforcer's `SELECT 1 FROM t WHERE id = $1 ...` existence
  check prepends the record id).
  """
  @spec shift_placeholders(String.t(), non_neg_integer()) :: String.t()
  def shift_placeholders(sql, 0) when is_binary(sql), do: sql

  def shift_placeholders(sql, offset) when is_binary(sql) and is_integer(offset) and offset > 0 do
    Regex.replace(~r/\$(\d+)/, sql, fn _whole, num ->
      "$#{String.to_integer(num) + offset}"
    end)
  end

  @doc """
  Inlines `$1..$n` placeholders of a compiled clause with escaped SQL literals.

  The compiler has no schema knowledge, so a bound parameter compared against
  a typed column (e.g. `\"id\" = $1` on a `uuid` column) crashes Postgrex's
  encoder or cannot encode `\"\"`. Inlining lets PostgreSQL resolve the literal
  type natively — the same semantics `Rules.Enforcer` already uses for the
  create-rule path. Values are single-quote-escaped; rules/filters are
  superuser-authored, and untyped literal comparisons (e.g. `\"id\" = ''`)
  become SQL errors that callers treat as "no match", not crashes.

  Note: this remains the escape hatch for callers without schema knowledge
  (e.g. user-provided list filters). The enforcer now binds parameters with
  explicit casts instead.
  """
  @spec inline_params(String.t(), [term()]) :: String.t()
  def inline_params(sql, params) when is_binary(sql) and is_list(params) do
    params
    |> Enum.with_index(1)
    |> Enum.reduce(sql, fn {val, idx}, acc ->
      val_str =
        cond do
          is_nil(val) -> "null"
          is_boolean(val) -> String.downcase(to_string(val))
          is_binary(val) -> ~s('#{escape_quote(val)}')
          true -> to_string(val)
        end

      String.replace(acc, "$#{idx}", val_str)
    end)
  end

  defp escape_quote(str), do: String.replace(str, "'", "''")

  # ── Tokenizer ────────────────────────────────────────

  defp tokenize(str) do
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

      {:ok, ast, rest} ->
        {:ok, ast, rest}

      error ->
        error
    end
  end

  defp parse_and(tokens) do
    case parse_not(tokens) do
      {:ok, left, ["&&" | rest]} ->
        case parse_and(rest) do
          {:ok, right, remaining} -> {:ok, {:and, left, right}, remaining}
          _ -> {:ok, left, rest}
        end

      {:ok, ast, rest} ->
        {:ok, ast, rest}

      error ->
        error
    end
  end

  defp parse_not(["!" | rest]) do
    case parse_comparison(rest) do
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
            value =
              case classify_literal(token) do
                {:param, n} -> {:param, n}
                literal -> {:literal, literal}
              end

            {:ok, {op, left, value}, rest}

          [] ->
            :error
        end

      {:ok, ast, rest} ->
        {:ok, ast, rest}

      error ->
        error
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
      {:param, n} -> {:ok, {:param, n}, rest}
      _ -> :error
    end
  end

  defp parse_primary([]), do: :error

  defp classify(token) do
    cond do
      token in ~w(true True TRUE) ->
        {:literal, true}

      token in ~w(false False FALSE) ->
        {:literal, false}

      token in ~w(null Null NULL) ->
        {:literal, nil}

      String.starts_with?(token, "'") and String.ends_with?(token, "'") ->
        {:literal, unescape_literal(token)}

      String.match?(token, ~r/^\d+(\.\d+)?$/) ->
        val =
          if String.contains?(token, "."), do: Decimal.new(token), else: String.to_integer(token)

        {:literal, val}

      String.match?(token, ~r/^\$(\d+)$/) ->
        {:param, String.to_integer(String.trim_leading(token, "$"))}

      String.match?(token, ~r/^[a-zA-Z_][a-zA-Z0-9_@]*$/) ->
        {:field, token}

      true ->
        :error
    end
  end

  defp classify_literal(token) do
    cond do
      token in ~w(true True TRUE) ->
        true

      token in ~w(false False FALSE) ->
        false

      token in ~w(null Null NULL) ->
        nil

      String.starts_with?(token, "'") and String.ends_with?(token, "'") ->
        unescape_literal(token)

      String.match?(token, ~r/^\d+(\.\d+)?$/) ->
        if String.contains?(token, "."), do: Decimal.new(token), else: String.to_integer(token)

      String.match?(token, ~r/^\$(\d+)$/) ->
        {:param, String.to_integer(String.trim_leading(token, "$"))}

      true ->
        token
    end
  end

  # SQL-style single-quoted literals unescape `''` → `'`. The enforcer's
  # escape_quote fallback emits this form for quote-containing token values,
  # so the bound parameter must carry the unescaped value to match a column
  # (or another literal) the same way the inline literal would.
  defp unescape_literal(token) do
    token
    |> String.slice(1, String.length(token) - 2)
    |> String.replace("''", "'")
  end

  # ── Type coercion pre-pass ───────────────────────────

  # Rewrites literal/param values that are compared against a column with a
  # known type into their Postgrex-encodable representation (e.g. a UUID string
  # into 16 bytes, `'5'` into a Decimal). Returns `:error` when the value
  # cannot be represented — callers treat that as "no match" (fail closed),
  # matching the previous behavior where the equivalent inline literal was a
  # PostgreSQL error.
  defp coerce_ast({op, {:field, f}, {:literal, val}}, types, _token_values)
       when op in ~w(= != > >= < <=) do
    case types[column_name(f)] do
      nil -> {:ok, {op, {:field, f}, {:literal, val}}}
      pg_type -> coerce_compare({op, {:field, f}, {:literal, val}}, pg_type, val)
    end
  end

  defp coerce_ast({op, {:field, f}, {:param, n}}, types, token_values)
       when op in ~w(= != > >= < <=) do
    case types[column_name(f)] do
      nil ->
        {:ok, {op, {:field, f}, {:param, n}}}

      pg_type ->
        coerce_compare({op, {:field, f}, {:param, n}}, pg_type, token_value(token_values, n))
    end
  end

  # ILIKE / NOT ILIKE only operate on text — the cast is always TEXT so the
  # bound parameter stays text-encodable, and a non-text column still fails
  # the query (denied) exactly like an inline literal would.
  defp coerce_ast({op, {:field, f}, {:literal, val}}, types, _token_values)
       when op in ~w(~ !~) do
    case types[column_name(f)] do
      nil -> {:ok, {op, {:field, f}, {:literal, val}}}
      _pg_type -> coerce_compare({op, {:field, f}, {:literal, val}}, "TEXT", val)
    end
  end

  defp coerce_ast({op, {:field, f}, {:param, n}}, types, token_values) when op in ~w(~ !~) do
    case types[column_name(f)] do
      nil ->
        {:ok, {op, {:field, f}, {:param, n}}}

      _pg_type ->
        coerce_compare({op, {:field, f}, {:param, n}}, "TEXT", token_value(token_values, n))
    end
  end

  defp coerce_ast({:or, left, right}, types, token_values) do
    with {:ok, left} <- coerce_ast(left, types, token_values),
         {:ok, right} <- coerce_ast(right, types, token_values) do
      {:ok, {:or, left, right}}
    end
  end

  defp coerce_ast({:and, left, right}, types, token_values) do
    with {:ok, left} <- coerce_ast(left, types, token_values),
         {:ok, right} <- coerce_ast(right, types, token_values) do
      {:ok, {:and, left, right}}
    end
  end

  defp coerce_ast({:not, expr}, types, token_values) do
    case coerce_ast(expr, types, token_values) do
      {:ok, expr} -> {:ok, {:not, expr}}
      :error -> :error
    end
  end

  defp coerce_ast(other, _types, _token_values), do: {:ok, other}

  defp coerce_compare(ast, pg_type, value) do
    case TypeMapper.coerce_value(pg_type, value) do
      {:ok, coerced} -> {:ok, replace_compare_value(ast, coerced)}
      :error -> :error
    end
  end

  defp replace_compare_value({op, left, _value}, coerced), do: {op, left, {:literal, coerced}}

  # Bound token value for a `{:param, n}` node. Accepts both the plain
  # `token_values` list (coercion pre-pass) and the `{types, token_values}`
  # emit context.
  defp token_value({_types, token_values}, n), do: Enum.at(token_values, n - 1, "")

  defp token_value(token_values, n) when is_list(token_values),
    do: Enum.at(token_values, n - 1, "")

  # ── Code generator ───────────────────────────────────

  defp emit({:or, left, right}, idx, ctx) do
    {l_sql, l_params, idx2} = emit_one(left, idx, ctx)
    {r_sql, r_params, _idx3} = emit_one(right, idx2, ctx)
    {"(#{l_sql} OR #{r_sql})", l_params ++ r_params}
  end

  defp emit({:and, left, right}, idx, ctx) do
    {l_sql, l_params, idx2} = emit_one(left, idx, ctx)
    {r_sql, r_params, _idx3} = emit_one(right, idx2, ctx)
    {"(#{l_sql} AND #{r_sql})", l_params ++ r_params}
  end

  defp emit({:not, expr}, idx, ctx) do
    {sql, params, _} = emit_one(expr, idx, ctx)
    {"NOT #{sql}", params}
  end

  defp emit(op_ast, idx, ctx) do
    {sql, params} = emit_simple(op_ast, ctx)
    {renumber(sql, idx), params}
  end

  # Field ILIKE 'pattern'
  defp emit_simple({"~", {:field, f}, {:literal, val}}, ctx) do
    {~s["#{column_name(f)}" ILIKE $1#{cast_for(f, ctx)}], ["%" <> val <> "%"]}
  end

  # Field ILIKE <bound param>
  defp emit_simple({"~", {:field, f}, {:param, n}}, ctx) do
    {~s["#{column_name(f)}" ILIKE $1#{cast_for(f, ctx)}], ["%" <> token_value(ctx, n) <> "%"]}
  end

  # Field NOT ILIKE 'pattern'
  defp emit_simple({"!~", {:field, f}, {:literal, val}}, ctx) do
    {~s["#{column_name(f)}" NOT ILIKE $1#{cast_for(f, ctx)}], ["%" <> val <> "%"]}
  end

  # Field NOT ILIKE <bound param>
  defp emit_simple({"!~", {:field, f}, {:param, n}}, ctx) do
    {~s["#{column_name(f)}" NOT ILIKE $1#{cast_for(f, ctx)}], ["%" <> token_value(ctx, n) <> "%"]}
  end

  # Field OP Literal
  defp emit_simple({op, {:field, f}, {:literal, val}}, ctx) when op in ~w(= != > >= < <=) do
    {~s["#{column_name(f)}" #{op} $1#{cast_for(f, ctx)}], [val]}
  end

  # Field OP Bound param
  defp emit_simple({op, {:field, f}, {:param, n}}, ctx) when op in ~w(= != > >= < <=) do
    {~s["#{column_name(f)}" #{op} $1#{cast_for(f, ctx)}], [token_value(ctx, n)]}
  end

  # Literal OP Literal — e.g. '' != '' (from @request.auth.id != '' when unauthenticated)
  defp emit_simple({op, {:literal, left}, {:literal, right}}, _ctx)
       when op in ~w(= != > >= < <=) do
    {~s[$1 #{op} $2], [left, right]}
  end

  # Literal OP Bound param — e.g. @request.auth.role = 'admin'
  defp emit_simple({op, {:literal, left}, {:param, n}}, ctx) when op in ~w(= != > >= < <=) do
    {~s[$1 #{op} $2], [left, token_value(ctx, n)]}
  end

  # Bound param OP Literal — e.g. 'admin' = @request.auth.role
  defp emit_simple({op, {:param, n}, {:literal, right}}, ctx) when op in ~w(= != > >= < <=) do
    {~s[$1 #{op} $2], [token_value(ctx, n), right]}
  end

  # Bound param OP Bound param
  defp emit_simple({op, {:param, a}, {:param, b}}, ctx) when op in ~w(= != > >= < <=) do
    {~s[$1 #{op} $2], [token_value(ctx, a), token_value(ctx, b)]}
  end

  # Standalone field
  defp emit_simple({:field, name}, _ctx) do
    {~s["#{column_name(name)}"], []}
  end

  # Standalone literal (fallback — unlikely)
  defp emit_simple({:literal, value}, _ctx) do
    {"$1", [value]}
  end

  # Standalone bound param
  defp emit_simple({:param, n}, ctx) do
    {"$1", [token_value(ctx, n)]}
  end

  # Catch-all: unknown AST node → no-op
  defp emit_simple(_ast, _ctx), do: {"", []}

  defp emit_one(ast, idx, ctx) do
    {sql, params} = emit(ast, idx, ctx)
    {sql, params, idx + length(params)}
  end

  defp renumber(sql, 1), do: sql

  # A clause can emit MULTIPLE placeholders (e.g. literal-vs-literal
  # comparisons emit "$1 op $2"). When compiled at index n > 1, EVERY
  # placeholder must be shifted by (n - 1) — rewriting only $1 leaves later
  # placeholders colliding with earlier clauses' params (silently wrong access
  # decisions) or produces a param/placeholder count mismatch that crashes
  # Postgres with "bind message supplies N parameters, but prepared statement
  # requires M" (HTTP 500 on create/view/update).
  defp renumber(sql, n) do
    shift_placeholders(sql, n - 1)
  end

  # Explicit Postgres cast for a field's placeholder, e.g. "::TEXT". Empty when
  # the column type is unknown (no schema knowledge).
  defp cast_for(field_name, {types, _token_values}) do
    case types[column_name(field_name)] do
      nil -> ""
      pg_type -> "::#{pg_type}"
    end
  end

  # Field identifiers in filters/rules reference the metadata name (which may
  # be mixed case, e.g. `tagColor`); DB columns are their lowercase form
  # (e.g. `tagcolor`), so emit the column name.
  defp column_name(name) when is_binary(name), do: String.downcase(name)
  defp column_name(name), do: name
end
