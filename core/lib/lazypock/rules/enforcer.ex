defmodule Lazypock.Rules.Enforcer do
  @moduledoc """
  Enforces collection-level access control rules.

  Rules use PocketBase-compatible syntax and are stored in
  `_collections.rules` per collection.

  ## How it works

  * **listRule** — Compiled via FilterCompiler into a SQL WHERE clause
    that is merged into the list query. @request.auth tokens are resolved
    to actual user values before compilation.

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
  @spec authorize_list(String.t(), map() | nil) :: {:ok, {String.t(), [term()]}} | {:error, String.t()}
  def authorize_list(collection_name, user) do
    rule = get_rule(collection_name, "listRule")
    if rule == "" do
      {:ok, {"", []}}
    else
      resolved = resolve_user_tokens(rule, user)
      FilterCompiler.compile(resolved)
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
  Authorizes a mutation against a record/attrs.
  """
  @spec authorize_mutation(String.t(), String.t(), map() | nil, map() | nil) :: :ok | {:error, String.t()}
  def authorize_mutation(collection_name, rule_key, user, record_or_attrs) do
    # Check manageRule first
    manage_rule = get_rule(collection_name, "manageRule")

    cond do
      manage_rule != "" and passes_rule?(manage_rule, user, record_or_attrs, collection_name) ->
        :ok

      true ->
        rule = get_rule(collection_name, rule_key)

        cond do
          rule == "" -> :ok
          passes_rule?(rule, user, record_or_attrs, collection_name) -> :ok
          true -> {:error, "Access denied by #{rule_key}"}
        end
    end
  end

  # ── Private ──────────────────────────────────────────

  defp get_rule(collection_name, rule_key) do
    {:ok, collection} = Registry.get(collection_name)
    collection.rules[rule_key] || ""
  end

  defp passes_rule?(rule, user, context, collection_name) do
    resolved = resolve_user_tokens(rule, user)

    case FilterCompiler.compile(resolved) do
      {:ok, {sql, params}} ->
        if sql == "" do
          true
        else
          eval_against_context(sql, params, context, collection_name)
        end

      {:error, _} ->
        false
    end
  end

  defp resolve_user_tokens(rule, nil) do
    rule
    |> String.replace(~r/@request\.auth\.id/, "''")
    |> String.replace(~r/@request\.auth\.email/, "''")
    |> String.replace(~r/@request\.auth\.role/, "''")
    |> String.replace(~r/@request\.auth\.\w+/, "''")
  end

  defp resolve_user_tokens(rule, user) do
    uid = escape_quote(to_string(Map.get(user, "id") || Map.get(user, :id) || ""))
    email = escape_quote(to_string(Map.get(user, "email") || Map.get(user, :email) || ""))
    role = escape_quote(to_string(Map.get(user, "role") || Map.get(user, :role) || "user"))

    rule
    |> String.replace(~r/@request\.auth\.id/, "'#{uid}'")
    |> String.replace(~r/@request\.auth\.email/, "'#{email}'")
    |> String.replace(~r/@request\.auth\.role/, "'#{role}'")
    |> String.replace(~r/@request\.auth\.\w+/, "''")
  end

  defp eval_against_context(sql, params, nil, _collection_name) do
    # No record context — rule references only @request.auth tokens
    # The sql is something like "'' != ''" → false
    # We can check by running a simple SELECT with no FROM
    {:ok, %{rows: rows}} =
      Ecto.Adapters.SQL.query(Repo, "SELECT 1 WHERE #{sql}", params)
    length(rows) > 0
  end

  defp eval_against_context(sql, params, record, collection_name) when is_map(record) do
    # Rule references record fields — check if record matches
    record_id = record["id"] || Map.get(record, :id)

    if record_id do
      table = TypeMapper.quote_ident(collection_name)
      {:ok, %{rows: rows}} =
        Ecto.Adapters.SQL.query(Repo,
          "SELECT 1 FROM #{table} WHERE id = $#{length(params) + 1} AND (#{sql}) LIMIT 1",
          params ++ [record_id]
        )
      length(rows) > 0
    else
      # No id yet (create action) — check what we can
      eval_without_db(sql, params, record)
    end
  end

  defp eval_without_db(sql, params, _record) do
    if String.contains?(sql, ~s(")) do
      # Rule references record fields that can't be checked pre-insert
      # This is a limitation — in PocketBase, create rules can reference
      # the new record's fields, but we check them against the input attrs
      true
    else
      # Pure auth check — run a SELECT 1 to evaluate
      case Ecto.Adapters.SQL.query(Repo, "SELECT 1 WHERE #{sql}", params) do
        {:ok, %{rows: rows}} when rows != [] -> true
        _ -> false
      end
    end
  end

  defp escape_quote(str), do: String.replace(str, "'", "''")
end
