defmodule Lazypock.Rules.Enforcer do
  @moduledoc """
  Enforces collection-level access control rules.

  Rules use PocketBase-compatible syntax and are stored in
  `_collections.rules` per collection.

  ## PocketBase-compatible rule semantics

  Rules follow three-state logic:

  | Rule value    | Meaning                                       |
  |---------------|-----------------------------------------------|
  | `nil` (absent)| **Superuser only.** Non-superusers denied.    |
  | `""` (empty)  | **Public.** Anyone can access (no filter).    |
  | filter string | **Conditional.** Users matching filter gain
                    access via `@request.auth.*` tokens.      |

  * **`@request.auth.*` tokens** — `@request.auth.id`, `@request.auth.email`,
    `@request.auth.role` are resolved to bound SQL parameters (`$N`) with
    explicit Postgres casts derived from each column's type via
    `Lazypock.Schema.TypeMapper`. For unauthenticated requests they become
    empty-string parameters.

  * **Superuser bypass** — Authenticated superusers always bypass all
    rules entirely (matches PocketBase superadmin behavior).

  ## How it works

  * **listRule** — Compiled via FilterCompiler into a SQL WHERE clause
    that is merged into the list query.

  * **viewRule, createRule, updateRule, deleteRule** — Resolved against the
    target record/attrs using a simple existence check:
    `SELECT 1 FROM collection WHERE id = $1 AND (<resolved_rule>)`
    Returns true if a row matches.

  * **manageRule** — If the user matches this rule, all other checks pass.
  """

  alias Lazypock.Collections.Registry
  alias Lazypock.Repo
  alias Lazypock.Schema.TypeMapper
  alias Lazypock.Schemas.FilterCompiler

  @doc """
  Returns SQL WHERE clause for list queries based on listRule.
  """
  @spec authorize_list(String.t(), map() | nil) ::
          {:ok, {String.t(), [term()]}} | {:error, String.t()}
  def authorize_list(collection_name, user) do
    # 1. Superuser bypass — authenticated superusers always have access
    if superuser?(user) do
      {:ok, {"", []}}
    else
      # 2. Check manageRule — admin-like access for non-superusers
      if manage_rule_matches?(collection_name, user, nil) do
        {:ok, {"", []}}
      else
        # 3. Check listRule — three-state plus a fail-closed bucket:
        #    nil = superuser only, "" = public, filter = evaluate,
        #    anything else (blank/invalid) = deny.
        case classify_rule(get_rule(collection_name, "listRule")) do
          :public ->
            {:ok, {"", []}}

          :filter ->
            rule = get_rule(collection_name, "listRule")

            case compile_rule(rule, user, collection_name) do
              {:ok, {sql, params}} ->
                # Rule values are bound as parameters with explicit casts
                # (e.g. "id" = $1::UUID) so Postgrex can encode them against
                # typed columns; the caller merges these into the list query.
                {:ok, {sql, params}}

              {:error, _} ->
                {:error, "Access denied by listRule"}
            end

          _ ->
            {:error, "Access denied by listRule"}
        end
      end
    end
  end

  @doc """
  Convenience: authorize_view against a single record.
  """
  @spec authorize_view(String.t(), map() | nil, map() | nil) :: :ok | {:error, String.t()}
  def authorize_view(collection_name, user, record) do
    authorize_mutation(collection_name, "viewRule", user, record)
  end

  @doc """
  Convenience: authorize_create against input attrs.
  """
  @spec authorize_create(String.t(), map() | nil, map()) :: :ok | {:error, String.t()}
  def authorize_create(collection_name, user, attrs) do
    authorize_mutation(collection_name, "createRule", user, attrs)
  end

  @doc """
  Convenience: authorize_update against existing record.
  """
  @spec authorize_update(String.t(), map() | nil, map()) :: :ok | {:error, String.t()}
  def authorize_update(collection_name, user, record) do
    authorize_mutation(collection_name, "updateRule", user, record)
  end

  @doc """
  Convenience: authorize_delete against existing record.
  """
  @spec authorize_delete(String.t(), map() | nil, map()) :: :ok | {:error, String.t()}
  def authorize_delete(collection_name, user, record) do
    authorize_mutation(collection_name, "deleteRule", user, record)
  end

  @doc """
  Convenience: authorize_manage for collection management access.
  Superusers always pass. Non-superusers must match the collection's
  manageRule (if set). If manageRule is nil, only superusers gain access.
  """
  @spec authorize_manage(String.t(), map() | nil) :: :ok | {:error, String.t()}
  def authorize_manage(collection_name, user) do
    authorize_mutation(collection_name, "manageRule", user, nil)
  end

  @doc """
  Authorizes a mutation against a record/attrs.
  """
  @spec authorize_mutation(String.t(), String.t(), map() | nil, map() | nil) ::
          :ok | {:error, String.t()}
  def authorize_mutation(collection_name, rule_key, user, record_or_attrs) do
    # 1. Superuser bypass — authenticated superusers always have access
    if superuser?(user) do
      :ok
    else
      # 2. Check manageRule — admin-like access for non-superusers
      if manage_rule_matches?(collection_name, user, record_or_attrs) do
        :ok
      else
        # 3. Check the specific action rule (viewRule, createRule, etc.)
        #    nil = superuser only, "" = public, filter = evaluate,
        #    anything else (blank/invalid) = deny.
        rule = get_rule(collection_name, rule_key)

        case classify_rule(rule) do
          :public ->
            :ok

          :filter ->
            if passes_rule?(rule, user, record_or_attrs, collection_name) do
              :ok
            else
              {:error, "Access denied by #{rule_key}"}
            end

          _ ->
            {:error, "Access denied by #{rule_key}"}
        end
      end
    end
  end

  # ── Private ──────────────────────────────────────────

  # Only a real superuser gets the bypass. Superusers arrive as
  # `%Lazypock.Auth.SuperUser{}` (see `Lazypock.Auth.Plug`), while
  # auth-collection users are plain maps from `GenericRecord.get/3`.
  #
  # This deliberately matches the concrete struct rather than any struct: the
  # previous `%{__struct__: _}` clause granted the bypass to *every* struct, so
  # any code path that passed an auth-collection user as an Ecto struct would
  # have silently escalated it to superuser.
  defp superuser?(%Lazypock.Auth.SuperUser{}), do: true
  defp superuser?(_), do: false

  # Three-state rule classification plus a fail-closed bucket:
  #
  #   * `:superuser_only` — `nil` / absent
  #   * `:public`         — exactly `""`
  #   * `:filter`         — a non-blank filter expression
  #   * `:invalid`        — whitespace-only, or a non-string value -> deny
  #
  # Whitespace-only rules must NOT be treated as `""` (public):
  # `FilterCompiler` trims its input, so `"   "` compiles to an empty clause,
  # and an empty clause means "no restriction" to the callers below. That
  # combination silently published a collection whose rule field only
  # contained spaces.
  defp classify_rule(nil), do: :superuser_only
  defp classify_rule(""), do: :public

  defp classify_rule(rule) when is_binary(rule) do
    if String.trim(rule) == "", do: :invalid, else: :filter
  end

  defp classify_rule(_other), do: :invalid

  # manageRule grants admin-like access when the user matches it.
  # `nil` = no delegation (superusers only); `""` = intentionally public
  # manage rule; blank/invalid never matches.
  defp manage_rule_matches?(collection_name, user, context) do
    rule = get_rule(collection_name, "manageRule")

    case classify_rule(rule) do
      :public -> true
      :filter -> passes_rule?(rule, user, context, collection_name)
      _ -> false
    end
  end

  defp get_rule(collection_name, rule_key) do
    {:ok, collection} = Registry.get(collection_name)
    collection.rules[rule_key]
  end

  defp passes_rule?(rule, user, context, collection_name) do
    case compile_rule(rule, user, collection_name) do
      # An empty clause never means "allow" here. Only the explicit `""`
      # (public) rule short-circuits to allow, and `classify_rule/1` handles
      # that before this function is reached.
      {:ok, {sql, params}} when sql != "" ->
        eval_against_context(sql, params, context, collection_name)

      _ ->
        false
    end
  end

  # Resolves @request.auth.* tokens, compiles the rule with schema-aware casts
  # and returns the bound SQL clause + params.
  defp compile_rule(rule, user, collection_name) do
    case resolve_user_tokens(rule, user) do
      {:ok, resolved, token_values} ->
        FilterCompiler.compile(resolved, token_values, field_types(collection_name))

      {:error, _reason} = error ->
        error
    end
  end

  # Column name (verbatim field name, matching FilterCompiler's emitted
  # identifiers) → PostgreSQL type, so the compiler can emit explicit casts
  # like `$1::UUID`.
  defp field_types(collection_name) do
    {:ok, collection} = Registry.get(collection_name)

    types =
      Map.new(collection.fields, fn field ->
        {field.name, TypeMapper.column_pg_type(field)}
      end)

    # Every collection has an `id` column. Base/auth collections expose a
    # system `id UUID` column that is not part of the user-defined fields, so
    # it is added here. View collections instead declare `id` as a (system,
    # text-typed) field whose physical column is TEXT (the view query CASTs it)
    # — that metadata entry is already in `types` and must be kept, because
    # emitting `$1::UUID` against a text column makes PostgreSQL fail with
    # "operator does not exist: text = uuid" (silently denials/empty lists).
    if Map.has_key?(types, "id") do
      types
    else
      Map.put(types, "id", TypeMapper.collection_id_pg_type(collection))
    end
  end

  # @request.auth.* tokens become `$N` placeholders whose values are bound as
  # parameters (in `token_values`, indexed 1..3). The returned triple feeds
  # FilterCompiler.compile/3.
  #
  # Only `id`, `email` and `role` are supported; any other `@request.auth.*`
  # token is rejected (`{:error, _}` -> deny). Unknown tokens used to be
  # substituted with an empty string, so a rule like `@request.auth.typo = ''`
  # evaluated to `'' = ''` (true) and granted access.
  defp resolve_user_tokens(rule, nil) do
    values = [nil, nil, nil]

    {sql, values} = apply_token(rule, ~r/@request\.auth\.id/, "", 1, values)
    {sql, values} = apply_token(sql, ~r/@request\.auth\.email/, "", 2, values)
    {sql, values} = apply_token(sql, ~r/@request\.auth\.role/, "", 3, values)

    reject_unknown_tokens(sql, values)
  end

  defp resolve_user_tokens(rule, user) do
    id = to_string(Map.get(user, "id") || Map.get(user, :id) || "")
    email = to_string(Map.get(user, "email") || Map.get(user, :email) || "")
    role = to_string(Map.get(user, "role") || Map.get(user, :role) || "user")

    values = [nil, nil, nil]

    {sql, values} = apply_token(rule, ~r/@request\.auth\.id/, id, 1, values)
    {sql, values} = apply_token(sql, ~r/@request\.auth\.email/, email, 2, values)
    {sql, values} = apply_token(sql, ~r/@request\.auth\.role/, role, 3, values)

    reject_unknown_tokens(sql, values)
  end

  defp reject_unknown_tokens(sql, values) do
    if String.contains?(sql, "@request.auth.") do
      {:error, "Unsupported @request.auth token in rule"}
    else
      {:ok, sql, values}
    end
  end

  # Replaces one token kind with a `$N` placeholder (bound param).
  #
  # A value containing a single quote keeps the escape_quote'd-literal
  # fallback: parameterizing the escaped form would change matching semantics
  # against SQL-escaped rule literals (e.g. `'o''brien@x.com'`), and the raw
  # value must never reach SQL. The escaped literal is the last-resort
  # defense-in-depth path — every other value is bound.
  defp apply_token(sql, regex, value, idx, values) do
    if String.contains?(value, "'") do
      {String.replace(sql, regex, ~s('#{escape_quote(value)}')), values}
    else
      {String.replace(sql, regex, "$#{idx}"), List.replace_at(values, idx - 1, value)}
    end
  end

  defp eval_against_context(sql, params, nil, _collection_name) do
    # No record context — rule references only @request.auth tokens.
    # The sql is something like "$1 != $2" → false for empty values.
    # A failing query (e.g. a typo'd field name) must deny, not crash.
    case Ecto.Adapters.SQL.query(Repo, "SELECT 1 WHERE #{sql}", params) do
      {:ok, %{rows: rows}} -> length(rows) > 0
      {:error, _} -> false
    end
  end

  defp eval_against_context(sql, params, record, collection_name) when is_map(record) do
    # Rule references record fields — check if record matches
    record_id = record["id"] || Map.get(record, :id)

    if record_id do
      table = TypeMapper.quote_ident(collection_name)

      # Postgrex needs values encoded against the *actual* column type of the
      # collection's `id` column: 16-byte binary for the system UUID column of
      # base/auth collections, but a plain string for view collections (whose
      # `id` column is TEXT, see Views.create_view/2). A malformed value (e.g.
      # from a caller passing a hand-built record map) must deny, not crash
      # with a 500 — the same fail-closed rule as query errors.
      id_type =
        case Registry.get(collection_name) do
          {:ok, collection} -> TypeMapper.collection_id_pg_type(collection)
          _ -> TypeMapper.id_column_type()
        end

      case TypeMapper.coerce_value(id_type, record_id) do
        {:ok, id_value} ->
          # The record id is bound as $1 with an explicit cast matching the
          # collection's id column type (uuid for base/auth, text for views).
          # Rule params are already cast/coerced by FilterCompiler, so the
          # rule's placeholders are shifted up by one to make room for $1.
          rule_sql = FilterCompiler.shift_placeholders(sql, 1)

          # A failing query (undefined column, bad expression) must deny, not
          # crash.
          case Ecto.Adapters.SQL.query(
                 Repo,
                 "SELECT 1 FROM #{table} WHERE id = $1::#{id_type} AND (#{rule_sql}) LIMIT 1",
                 [id_value | params]
               ) do
            {:ok, %{rows: rows}} -> length(rows) > 0
            {:error, _} -> false
          end

        :error ->
          false
      end
    else
      # No id yet (create action) — check what we can
      eval_without_db(sql, params, record)
    end
  end

  defp eval_without_db(sql, params, record) do
    if String.contains?(sql, ~s(")) do
      # Rule references record fields — resolve them against the input attrs
      resolved =
        Enum.reduce(record, sql, fn {key, val}, acc ->
          key_str = if is_atom(key), do: Atom.to_string(key), else: key

          val_str =
            cond do
              is_nil(val) -> "null"
              is_boolean(val) -> String.downcase(to_string(val))
              is_binary(val) -> ~s('#{escape_quote(val)}')
              true -> to_string(val)
            end

          String.replace(acc, ~s("#{key_str}"), val_str)
        end)

      # Resolve remaining $1, $2 etc. parameter placeholders with actual values
      # (params may already be coerced to their column representation, e.g. a
      # 16-byte uuid binary or a Decimal)
      resolved =
        params
        |> Enum.with_index(1)
        |> Enum.reduce(resolved, fn {val, idx}, acc ->
          String.replace(acc, "$#{idx}", inline_sql_value(val))
        end)

      # Now run SELECT 1 WHERE with fully resolved values
      case Ecto.Adapters.SQL.query(Repo, "SELECT 1 WHERE #{resolved}", []) do
        {:ok, %{rows: rows}} when rows != [] -> true
        _ -> false
      end
    else
      # Pure auth check — run a SELECT 1 to evaluate
      case Ecto.Adapters.SQL.query(Repo, "SELECT 1 WHERE #{sql}", params) do
        {:ok, %{rows: rows}} when rows != [] -> true
        _ -> false
      end
    end
  end

  # Formats a value as a SQL literal for eval_without_db's inline evaluation,
  # handling values already coerced by TypeMapper (uuid binaries, Decimals,
  # DateTimes).
  defp inline_sql_value(nil), do: "null"

  defp inline_sql_value(val) when is_boolean(val), do: String.downcase(to_string(val))

  defp inline_sql_value(val) when is_binary(val) and byte_size(val) == 16 do
    case Ecto.UUID.load(val) do
      {:ok, uuid} -> ~s('#{uuid}')
      _ -> "null"
    end
  end

  defp inline_sql_value(val) when is_binary(val), do: ~s('#{escape_quote(val)}')
  defp inline_sql_value(%Decimal{} = val), do: to_string(val)
  defp inline_sql_value(%DateTime{} = val), do: ~s('#{DateTime.to_iso8601(val)}')
  defp inline_sql_value(val), do: to_string(val)

  defp escape_quote(str), do: String.replace(str, "'", "''")
end
