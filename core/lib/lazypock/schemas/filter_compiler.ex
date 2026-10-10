defmodule Lazypock.Schemas.FilterCompiler do
  @moduledoc """
  Compiles PocketBase-compatible filter syntax into safe SQL WHERE clauses
  with parameterized values.
  """

  alias Lazypock.Schema.TypeMapper
  alias Lazypock.Schemas.FilterCompiler.Cache

  # Standard comparison operators.
  @standard_ops ~w(= != ~ !~ > >= < <=)

  # PocketBase "?" prefix operators — "any/at least one of" conditions over
  # array-valued fields (multi_select / multi_file / multi-relation → TEXT[]).
  @array_ops ["?=", "?!=", "?~", "?!~", "?>", "?>=", "?<", "?<="]

  @comparison_ops @standard_ops ++ @array_ops

  # PocketBase parity (tools/search/provider.go): filters longer than
  # MaxFilterLength (3500) or with more than DefaultFilterExprLimit (200)
  # expressions are rejected, bounding parser work for user-supplied
  # `?filter=` values as well as superuser-authored rules.
  @max_filter_length 3500
  @max_filter_expressions 200

  # Parsed-AST memo cache, owned by `Lazypock.Schemas.FilterCompiler.Cache` —
  # a supervised process, so the table is not owned (and destroyed) by whichever
  # caller happens to compile a filter first. That module documents why the AST
  # needs no invalidation; a missing table degrades to a cache miss.

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
  @spec compile(String.t(), [term()], map(), map()) ::
          {:ok, {String.t(), [term()]}} | {:error, String.t()}
  def compile(filter_str, token_values \\ [], types \\ %{}, opts \\ %{})
      when is_binary(filter_str) and is_list(token_values) and is_map(types) and is_map(opts) do
    filter_str = String.trim(filter_str)

    cond do
      filter_str == "" ->
        {:ok, {"", []}}

      byte_size(filter_str) > @max_filter_length ->
        {:error, "Filter exceeds the maximum length of #{@max_filter_length} characters"}

      true ->
        case parse_cached(filter_str) do
          {:ok, ast} ->
            case resolve_relations(ast, opts) do
              {:ok, ast} ->
                if count_expressions(ast) > @max_filter_expressions do
                  {:error, "Filter exceeds the maximum of #{@max_filter_expressions} expressions"}
                else
                  build_expr(ast, types, token_values, opts)
                end

              {:error, reason} ->
                {:error, reason}
            end

          :error ->
            {:error, "Failed to parse filter expression"}
        end
    end
  end

  # Returns the parsed AST for `filter_str`, memoized by `Cache`. Caches only
  # successful parses that consume all tokens; parse failures are re-evaluated
  # (and so remain cheap to reject without unbounded cache growth from junk).
  defp parse_cached(filter_str) do
    case Cache.lookup(filter_str) do
      {:ok, ast} ->
        {:ok, ast}

      :miss ->
        case parse(filter_str) do
          {:ok, ast} ->
            Cache.put(filter_str, ast)
            {:ok, ast}

          :error ->
            :error
        end
    end
  end

  defp parse(filter_str) do
    with {:ok, tokens} <- tokenize(filter_str),
         {:ok, ast, []} <- parse_or(tokens) do
      {:ok, ast}
    else
      _ -> :error
    end
  end

  @doc false
  def clear_cache, do: Cache.clear()

  @doc false
  def cache_size, do: Cache.size()

  @doc false
  def cached?(filter_str) when is_binary(filter_str) do
    Cache.member?(String.trim(filter_str))
  end

  defp build_expr(ast, types, token_values, opts) do
    with {:ok, coerced_ast} <- coerce_ast(ast, types, token_values),
         {:ok, coerced_ast} <- coerce_rels(coerced_ast, token_values, opts) do
      ctx = %{types: types, token_values: token_values, opts: opts}
      {sql, params} = emit(coerced_ast, 1, ctx)

      # A non-empty filter that compiles to an empty clause means the AST was
      # never turned into SQL -- e.g. a comparison with no field operand such
      # as `'a' ~ 'b'` or `$1 ?= 'b'`, which the code generator's catch-all
      # swallows. Callers treat an empty clause as "no restriction" (the
      # public-`""` fast path), so returning `{:ok, {"", []}}` here would
      # grant unconditional access. Fail closed instead.
      if sql == "" do
        {:error, "Filter does not reference a field and cannot be evaluated"}
      else
        {:ok, {sql, params}}
      end
    else
      :error -> {:error, "Rule value cannot be represented as its column type"}
    end
  end

  # ── Relation dot-paths ────────────────────────────────
  #
  # PocketBase allows filtering on a related record's field with a dot-path
  # (`author.email = 'x'`). The compiler can't extend the caller's FROM clause,
  # so each hop is emitted as a correlated `EXISTS (SELECT 1 FROM target ...)`
  # subquery, which composes safely with the existing WHERE clause and bound
  # parameters. Resolution needs the source collection (for the outer
  # correlation) and schema metadata; both arrive via `opts`:
  #
  #     %{source: "posts", resolver: fn coll -> {:ok, %{types: .., relations: ..}} end}
  #
  # `resolver` defaults to the live collection Registry. Tests can inject a
  # pure resolver so dot-path compilation is covered without a database.

  defp resolve_relations({:or, left, right}, opts) do
    with {:ok, left} <- resolve_relations(left, opts),
         {:ok, right} <- resolve_relations(right, opts) do
      {:ok, {:or, left, right}}
    end
  end

  defp resolve_relations({:and, left, right}, opts) do
    with {:ok, left} <- resolve_relations(left, opts),
         {:ok, right} <- resolve_relations(right, opts) do
      {:ok, {:and, left, right}}
    end
  end

  defp resolve_relations({:not, expr}, opts) do
    case resolve_relations(expr, opts) do
      {:ok, expr} -> {:ok, {:not, expr}}
      error -> error
    end
  end

  defp resolve_relations({op, {:field, name}, value}, opts) when is_binary(name) do
    if String.contains?(name, ".") do
      with {:ok, hops, leaf, leaf_type} <- resolve_dotted(name, opts) do
        {:ok, {:rel, hops, leaf, leaf_type, op, value}}
      end
    else
      {:ok, {op, {:field, name}, value}}
    end
  end

  defp resolve_relations(other, _opts), do: {:ok, other}

  defp resolve_dotted(name, opts) do
    case Map.get(opts, :source) do
      nil -> {:error, "Relation field path requires a source collection: #{name}"}
      source -> resolve_segments(String.split(name, "."), source, [], opts)
    end
  end

  # Final segment — a leaf field on the current (target) collection.
  defp resolve_segments([leaf], collection, hops, opts) do
    case fetch_schema(collection, opts) do
      {:ok, schema} ->
        {:ok, Enum.reverse(hops), leaf, Map.get(schema.types, leaf)}

      error ->
        error
    end
  end

  # A relation hop — follow it into the target collection.
  defp resolve_segments([field | rest], collection, hops, opts) do
    with {:ok, schema} <- fetch_schema(collection, opts),
         {:ok, rel} <- fetch_relation(schema, field) do
      hop = %{field: field, target: rel.target, multi: rel.multi}
      resolve_segments(rest, rel.target, [hop | hops], opts)
    end
  end

  defp fetch_schema(collection, opts) do
    resolver = Map.get(opts, :resolver) || (&default_schema/1)
    resolver.(collection)
  end

  defp fetch_relation(schema, field) do
    case Map.get(schema.relations, field) do
      %{target: target} = rel when is_binary(target) -> {:ok, rel}
      _ -> {:error, "Unknown relation field on path: #{field}"}
    end
  end

  # Live schema lookup (collection name → types + relation targets).
  defp default_schema(collection_name) do
    case Lazypock.Collections.Registry.get(collection_name) do
      {:ok, collection} ->
        fields = collection.fields || []

        types =
          fields
          |> Map.new(fn field -> {field.name, TypeMapper.column_pg_type(field)} end)
          |> Map.put_new("id", TypeMapper.collection_id_pg_type(collection))

        relations =
          for field <- fields,
              field.type == "relation",
              target = field.options["collection"],
              is_binary(target),
              into: %{} do
            {field.name, %{target: target, multi: (field.options["maxSelect"] || 1) > 1}}
          end

        {:ok, %{types: types, relations: relations}}

      {:error, _} ->
        {:error, "Unknown collection: #{collection_name}"}
    end
  end

  # Counts comparison nodes. PocketBase applies its expression limit per parsed
  # filter expression; mirror that so deeply-nested input cannot force
  # unbounded code generation.
  defp count_expressions({:or, left, right}),
    do: count_expressions(left) + count_expressions(right)

  defp count_expressions({:and, left, right}),
    do: count_expressions(left) + count_expressions(right)

  defp count_expressions({:not, expr}), do: count_expressions(expr)

  defp count_expressions({op, _left, _right}) when op in @comparison_ops, do: 1
  defp count_expressions(_other), do: 1

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

  # ── LIKE pattern building (PocketBase wrapLikeParams parity) ──

  # Wraps a value in `%...%` for contains semantics, escaping the LIKE
  # metacharacters so they match literally. A value containing an explicit
  # unescaped `%` is an author-supplied pattern and is returned untouched.
  defp like_pattern(nil), do: nil

  defp like_pattern(val) when is_binary(val) do
    if contains_unescaped_percent?(val) do
      val
    else
      "%" <> escape_like_chars(val) <> "%"
    end
  end

  defp like_pattern(val), do: like_pattern(to_string(val))

  # Escapes `%`, `_` and `\` when they are not already escaping something.
  # Mirrors PocketBase's `escapeUnescapedChars(v, '\\', '%', '_')`: existing
  # escape sequences are preserved instead of double-escaped.
  #
  # Byte-wise on purpose: `String.to_charlist/1` raises
  # `UnicodeConversionError` on invalid UTF-8, so a filter value containing
  # arbitrary bytes would have crashed the compiler (a 500 on `?filter=`). The
  # metacharacters are single ASCII bytes, so multi-byte UTF-8 sequences are
  # unaffected.
  defp escape_like_chars(str) do
    str
    |> :binary.bin_to_list()
    |> escape_like_chars([])
    |> :erlang.list_to_binary()
  end

  defp escape_like_chars([], acc), do: Enum.reverse(acc)

  defp escape_like_chars([?\\, next | rest], acc) do
    escape_like_chars(rest, [next, ?\\ | acc])
  end

  defp escape_like_chars([c | rest], acc) when c in [?%, ?_] do
    escape_like_chars(rest, [c, ?\\ | acc])
  end

  defp escape_like_chars([c | rest], acc), do: escape_like_chars(rest, [c | acc])

  # Mirrors PocketBase's `containsUnescapedChar(str, '%')`, including the
  # `\\`-resets-the-escape-state rule. Byte-wise for the same reason as
  # `escape_like_chars/1`.
  defp contains_unescaped_percent?(str) do
    contains_unescaped_percent?(:binary.bin_to_list(str), nil)
  end

  defp contains_unescaped_percent?([], _prev), do: false
  defp contains_unescaped_percent?([?% | _rest], prev) when prev != ?\\, do: true
  defp contains_unescaped_percent?([?\\ | rest], ?\\), do: contains_unescaped_percent?(rest, nil)
  defp contains_unescaped_percent?([c | rest], _prev), do: contains_unescaped_percent?(rest, c)

  # ── Tokenizer ────────────────────────────────────────

  defp tokenize(str) do
    tokens =
      Regex.split(
        # Longest match first: the "?" operators must be tried before their
        # shorter "?" / bare-operator prefixes (e.g. `?>=` before `?>`).
        ~r/(&&|\|\||\?>=|\?<=|\?!=|\?!~|\?=|\?~|\?>|\?<|>=|<=|!=|!~|>|<|~|=|!|[()])/,
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
          {:ok, right, remaining} ->
            {:ok, {:or, left, right}, remaining}

          # A dangling `||` with no parsable right operand must be an error.
          # The previous `{:ok, left, rest}` fallback silently dropped the
          # operator, so `a = 1 ||` compiled as if it were `a = 1`.
          _ ->
            :error
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
          {:ok, right, remaining} ->
            {:ok, {:and, left, right}, remaining}

          # Same as `||` above: a dangling `&&` must not silently vanish.
          _ ->
            :error
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
      {:ok, left, [op | value_tokens]} when op in @comparison_ops ->
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

      String.match?(token, ~r/^[a-zA-Z_][a-zA-Z0-9_@]*(\.[a-zA-Z_][a-zA-Z0-9_]*)*$/) ->
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
    inner_length = String.length(token) - 2

    # `classify/1` treats any token that both starts and ends with `'` as a
    # quoted literal -- including a lone `'`, whose computed inner length is
    # -1. `String.slice/3` raises FunctionClauseError on a negative length, so
    # a filter containing a bare quote used to crash the compiler (a 500 on
    # `?filter=`).
    if inner_length > 0 do
      token
      |> String.slice(1, inner_length)
      |> String.replace("''", "'")
    else
      ""
    end
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

  # PocketBase "?" prefix operators ("any/at least one of") apply to
  # array-valued columns. Only TEXT[] is supported: the value is coerced to
  # its text representation so `= ANY` / `ILIKE ANY` compare text-to-text.
  # A known non-array column (scalar or JSONB) means the operator doesn't
  # apply → fail closed, matching the inline-literal behavior.
  defp coerce_ast({op, {:field, f}, {:literal, val}}, types, _token_values)
       when op in @array_ops do
    array_coerce(op, f, val, types)
  end

  defp coerce_ast({op, {:field, f}, {:param, n}}, types, token_values)
       when op in @array_ops do
    array_coerce(op, f, token_value(token_values, n), types)
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

  # Relation dot-path leaves are coerced in a separate pass (after the plain
  # fields) so the `inline` option can skip it: the API `?filter=` path inlines
  # parameters as SQL literals, where a coerced 16-byte UUID binary would be
  # inlined as a broken byte string. Leaving the original string lets the
  # emitted `::UUID` cast resolve it (`'…uuid…'::UUID`).
  defp coerce_rels({:or, left, right}, token_values, opts) do
    with {:ok, left} <- coerce_rels(left, token_values, opts),
         {:ok, right} <- coerce_rels(right, token_values, opts) do
      {:ok, {:or, left, right}}
    end
  end

  defp coerce_rels({:and, left, right}, token_values, opts) do
    with {:ok, left} <- coerce_rels(left, token_values, opts),
         {:ok, right} <- coerce_rels(right, token_values, opts) do
      {:ok, {:and, left, right}}
    end
  end

  defp coerce_rels({:not, expr}, token_values, opts) do
    case coerce_rels(expr, token_values, opts) do
      {:ok, expr} -> {:ok, {:not, expr}}
      error -> error
    end
  end

  defp coerce_rels({:rel, hops, leaf, leaf_type, op, value}, token_values, opts) do
    if Map.get(opts, :inline, false) do
      {:ok, {:rel, hops, leaf, leaf_type, op, value}}
    else
      rel_types = if leaf_type, do: %{leaf => leaf_type}, else: %{}

      case coerce_ast({op, {:field, leaf}, value}, rel_types, token_values) do
        {:ok, {op2, {:field, ^leaf}, coerced}} ->
          {:ok, {:rel, hops, leaf, leaf_type, op2, coerced}}

        :error ->
          :error
      end
    end
  end

  defp coerce_rels(other, _token_values, _opts), do: {:ok, other}

  defp coerce_compare(ast, pg_type, value) do
    case TypeMapper.coerce_value(pg_type, value) do
      {:ok, coerced} -> {:ok, replace_compare_value(ast, coerced)}
      :error -> :error
    end
  end

  defp replace_compare_value({op, left, _value}, coerced), do: {op, left, {:literal, coerced}}

  defp array_coerce(op, f, val, types) do
    case types[column_name(f)] do
      nil -> {:ok, {op, {:field, f}, {:literal, val}}}
      "TEXT[]" -> {:ok, {op, {:field, f}, {:literal, to_text_value(val)}}}
      _ -> :error
    end
  end

  # Element representation for a TEXT[] column comparison. Filter literals are
  # strings/numbers/booleans/Decimals; `= ANY` / `ILIKE ANY` need text on both
  # sides so the bound value must match the array's element type.
  defp to_text_value(v) when is_binary(v), do: v
  defp to_text_value(v) when is_integer(v), do: Integer.to_string(v)
  defp to_text_value(v) when is_boolean(v), do: to_string(v)
  defp to_text_value(%Decimal{} = v), do: Decimal.to_string(v)
  defp to_text_value(v) when is_float(v), do: to_string(v)
  defp to_text_value(nil), do: nil
  defp to_text_value(v), do: to_string(v)

  # Bound token value for a `{:param, n}` node. Accepts both the plain
  # `token_values` list (coercion pre-pass) and the `{types, token_values}`
  # emit context.
  defp token_value(%{token_values: token_values}, n), do: Enum.at(token_values, n - 1, "")

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
  #
  # The pattern goes through `like_pattern/1` (PocketBase `wrapLikeParams`):
  # `%`, `_` and `\` in the value are escaped so they match literally, the
  # value is wrapped in `%...%` for contains semantics, and `ESCAPE '\'`
  # declares the escape character. A value containing an explicit unescaped
  # `%` is treated as an author-supplied pattern and left untouched.
  defp emit_simple({"~", {:field, f}, {:literal, val}}, ctx) do
    {like_clause(f, ctx, "ILIKE"), [like_pattern(val)]}
  end

  # Field ILIKE <bound param>
  defp emit_simple({"~", {:field, f}, {:param, n}}, ctx) do
    {like_clause(f, ctx, "ILIKE"), [like_pattern(token_value(ctx, n))]}
  end

  # Field NOT ILIKE 'pattern'
  defp emit_simple({"!~", {:field, f}, {:literal, val}}, ctx) do
    {like_clause(f, ctx, "NOT ILIKE"), [like_pattern(val)]}
  end

  # Field NOT ILIKE <bound param>
  defp emit_simple({"!~", {:field, f}, {:param, n}}, ctx) do
    {like_clause(f, ctx, "NOT ILIKE"), [like_pattern(token_value(ctx, n))]}
  end

  # Field = null / != null → IS NULL / IS NOT NULL.
  #
  # A SQL `col = NULL` comparison evaluates to UNKNOWN (never true), so it can
  # never match — the opposite of the PocketBase-compatible intent, where
  # `deleted_at = null` selects rows with no value. Emit the IS [NOT] NULL form
  # (no bound parameter) for both the literal and bound-param shapes.
  defp emit_simple({op, {:field, f}, {:literal, nil}}, _ctx) when op in ~w(= !=) do
    {nil_clause(f, op), []}
  end

  defp emit_simple({op, {:field, f}, {:param, n}}, ctx) when op in ~w(= !=) do
    case token_value(ctx, n) do
      nil -> {nil_clause(f, op), []}
      value -> {~s["#{column_name(f)}" #{op} $1#{cast_for(f, ctx)}], [value]}
    end
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

  # PocketBase "?" prefix operators — array ("any/at least one of") conditions.
  # The value is already coerced to its text form by `coerce_ast/3`. `ANY`
  # OR-combines the per-element predicate; the negated forms use
  # `NOT (<predicate> ALL(...))`, which is true when at least one element
  # fails the predicate (PocketBase's "any/at least one of NOT ...").
  defp emit_simple({op, {:field, f}, {:literal, val}}, _ctx) when op in @array_ops do
    col = column_name(f)

    case op do
      # ILIKE needs the array element as the *left* operand and the pattern on
      # the right, so `$1 ILIKE ANY(col)` would reverse them. `unnest` + EXISTS
      # keeps the element/pattern order correct.
      "?~" ->
        {~s[EXISTS (SELECT 1 FROM unnest("#{col}") AS x WHERE x ILIKE $1 ESCAPE '\\')],
         [like_pattern(val)]}

      "?!~" ->
        {~s[EXISTS (SELECT 1 FROM unnest("#{col}") AS x WHERE x NOT ILIKE $1 ESCAPE '\\')],
         [like_pattern(val)]}

      _ ->
        {cmp, negate} = array_op(op)

        if negate do
          {~s[NOT ($1 #{cmp} ALL("#{col}"))], [val]}
        else
          {~s[$1 #{cmp} ANY("#{col}")], [val]}
        end
    end
  end

  # Relation dot-path — a correlated EXISTS per hop. The leaf predicate is
  # built against the innermost alias, then wrapped outward so each hop joins
  # its target's `id` to the previous relation column (ANY(...) for multi).
  defp emit_simple({:rel, hops, leaf, leaf_type, op, value}, ctx) do
    col = "#{rel_alias(length(hops) - 1)}.#{TypeMapper.quote_ident(leaf)}"
    {predicate, params} = rel_predicate(op, col, leaf_type, value, ctx)
    {build_rel_exists(hops, predicate, ctx.opts[:source]), params}
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

  # Map a "?" operator to its per-element predicate: {sql_cmp, negate}.
  # `negate` turns the positive `x ANY` form into `NOT (x ALL)`, i.e.
  # "at least one fails". The ILIKE forms (`?~` / `?!~`) are handled
  # separately because `ANY` would put the pattern on the wrong side.
  defp array_op("?="), do: {"=", false}
  defp array_op("?!="), do: {"=", true}
  defp array_op("?>"), do: {"<", false}
  defp array_op("?>="), do: {"<=", false}
  defp array_op("?<"), do: {">", false}
  defp array_op("?<="), do: {">=", false}

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

  # `<col> IS [NOT] NULL`
  defp nil_clause(field_name, "="), do: ~s["#{column_name(field_name)}" IS NULL]
  defp nil_clause(field_name, "!="), do: ~s["#{column_name(field_name)}" IS NOT NULL]

  # ── Relation dot-path SQL ────────────────────────────

  defp rel_alias(index), do: TypeMapper.quote_ident("lp_r#{index}")

  # Leaf predicate against the innermost EXISTS alias. Mirrors the plain field
  # emitters, but qualifies the column with the alias.
  defp rel_predicate(op, col, _leaf_type, value, ctx) when op in ~w(~ !~) do
    sql_op = if op == "~", do: "ILIKE", else: "NOT ILIKE"
    {~s[#{col} #{sql_op} $1::TEXT ESCAPE '\\'], [like_pattern(rel_value(value, ctx))]}
  end

  defp rel_predicate(op, col, _leaf_type, value, ctx) when op in @array_ops do
    val = rel_value(value, ctx)

    case op do
      "?~" ->
        {~s[EXISTS (SELECT 1 FROM unnest(#{col}) AS x WHERE x ILIKE $1 ESCAPE '\\')],
         [like_pattern(val)]}

      "?!~" ->
        {~s[EXISTS (SELECT 1 FROM unnest(#{col}) AS x WHERE x NOT ILIKE $1 ESCAPE '\\')],
         [like_pattern(val)]}

      _ ->
        {cmp, negate} = array_op(op)

        if negate do
          {~s[NOT ($1 #{cmp} ALL(#{col}))], [val]}
        else
          {~s[$1 #{cmp} ANY(#{col})], [val]}
        end
    end
  end

  defp rel_predicate(op, col, leaf_type, value, ctx) when op in ~w(= != > >= < <=) do
    val = rel_value(value, ctx)

    if val == nil and op in ~w(= !=) do
      {"#{col} #{if op == "=", do: "IS NULL", else: "IS NOT NULL"}", []}
    else
      {~s[#{col} #{op} $1#{rel_cast(leaf_type)}], [val]}
    end
  end

  defp rel_value({:literal, value}, _ctx), do: value
  defp rel_value({:param, n}, ctx), do: token_value(ctx, n)

  defp rel_cast(nil), do: ""
  defp rel_cast(pg_type), do: "::#{pg_type}"

  # Wrap the leaf predicate in one correlated EXISTS per hop, innermost first.
  defp build_rel_exists(hops, predicate, source) do
    hops
    |> Enum.with_index()
    |> Enum.reverse()
    |> Enum.reduce(predicate, fn {hop, index}, acc ->
      alias_i = rel_alias(index)

      lhs =
        if index == 0 do
          ref = "#{TypeMapper.quote_ident(source)}.#{TypeMapper.quote_ident(hop.field)}"
          if hop.multi, do: "ANY(#{ref})", else: ref
        else
          ref = "#{rel_alias(index - 1)}.#{TypeMapper.quote_ident(hop.field)}"
          if hop.multi, do: "ANY(#{ref})", else: ref
        end

      table = TypeMapper.quote_ident(hop.target)

      # Relation columns are TEXT (or TEXT[] for multi), while the target id is
      # UUID (TEXT for views). Compare as text on both sides.
      "EXISTS (SELECT 1 FROM #{table} AS #{alias_i} WHERE #{alias_i}.\"id\"::TEXT = #{lhs} AND #{acc})"
    end)
  end

  # `<col> [NOT] ILIKE $1<cast> ESCAPE '\'`
  defp like_clause(field_name, ctx, op) do
    ~s["#{column_name(field_name)}" #{op} $1#{cast_for(field_name, ctx)} ESCAPE '\\']
  end

  # Explicit Postgres cast for a field's placeholder, e.g. "::TEXT". Empty when
  # the column type is unknown (no schema knowledge).
  defp cast_for(field_name, %{types: types}) do
    case types[column_name(field_name)] do
      nil -> ""
      pg_type -> "::#{pg_type}"
    end
  end

  # Field identifiers in filters/rules reference the metadata name (which may
  # be mixed case, e.g. `tagColor`). That name is kept verbatim as the
  # physical column (quoted identifiers preserve case), so emit it unchanged.
  defp column_name(name) when is_binary(name), do: name
  defp column_name(name), do: to_string(name)
end
