defmodule Lazypock.Files.Rules do
  @moduledoc """
  Access rules for the file library (`_files`), mirroring collection rules.

  Rules live in `_settings.data["files"]["rules"]` as a map that may contain
  `listRule` and/or `deleteRule`:

  ```json
  { "files": { "rules": {
      "listRule":   "@request.auth.id = uploaded_by || @request.auth.role = 'admin'",
      "deleteRule": "@request.auth.id = uploaded_by || @request.auth.role = 'admin'"
  } } }
  ```

  ## Three-state semantics (identical to `Lazypock.Rules.Enforcer`)

  | Rule value      | Meaning                                    |
  |-----------------|--------------------------------------------|
  | `nil` / absent  | **Superuser only.** Non-superusers denied. |
  | `""` (empty)    | **Public.** Anyone may access.             |
  | filter string   | **Conditional**, evaluated per file row.   |

  Whitespace-only or non-string values are **invalid** and deny (fail closed).

  ## Ownership

  Every upload records the uploading identity in `_files.uploaded_by` (the
  auth-collection user id, or the superuser id), so a single global rule such
  as `@request.auth.id = uploaded_by` gives per-file behaviour: users can list
  and delete the files *they* uploaded, while the rule set itself stays global.

  `@request.auth.id`, `@request.auth.email` and `@request.auth.role` resolve
  exactly as they do for collection rules. Available columns include
  `uploaded_by`, `filename`, `mime_type`, `extension`, `size`, `origin`,
  `status`, `collection_name`, `field_name`, `record_id` and `created_at`.

  **Superusers always bypass every file rule** — the Studio media library keeps
  working unchanged.
  """

  alias Lazypock.Repo
  alias Lazypock.Rules.Enforcer
  alias Lazypock.Schema.TypeMapper
  alias Lazypock.Schemas.FilterCompiler
  alias Lazypock.Settings

  # `_files` column name => PostgreSQL type, so FilterCompiler can emit casts
  # (`"uploaded_by" = $1::TEXT`) and coerce bound values the way it does for
  # collection fields.
  @field_types %{
    "id" => "UUID",
    "filename" => "TEXT",
    "extension" => "TEXT",
    "mime_type" => "TEXT",
    "size" => "NUMERIC",
    "storage_backend" => "TEXT",
    "collection_name" => "TEXT",
    "record_id" => "TEXT",
    "field_name" => "TEXT",
    "thumbs" => "JSONB",
    "variants" => "JSONB",
    "status" => "TEXT",
    "original_name" => "TEXT",
    "width" => "NUMERIC",
    "height" => "NUMERIC",
    "checksum" => "TEXT",
    "origin" => "TEXT",
    "uploaded_by" => "TEXT",
    "attached_at" => "TIMESTAMPTZ",
    "created_at" => "TIMESTAMPTZ",
    "updated_at" => "TIMESTAMPTZ"
  }

  @doc "The valid `_files` columns and their PostgreSQL types (for rule editors)."
  @spec field_types() :: %{optional(String.t()) => String.t()}
  def field_types, do: @field_types

  @doc "The configured file rules map (empty map when nothing is set)."
  @spec rules() :: map()
  def rules do
    case Settings.get("files", %{}) do
      %{"rules" => rules} when is_map(rules) -> rules
      _ -> %{}
    end
  rescue
    _ -> %{}
  end

  @doc "True when the identity is a superuser (bypasses every file rule)."
  @spec superuser?(map() | nil) :: boolean()
  def superuser?(user), do: Enforcer.superuser?(user)

  @doc """
  Authorizes listing files.

  Returns `{:ok, {sql, params}}` where `sql` is a WHERE fragment to merge into
  the list query (`{"", []}` means no restriction), or `{:error, message}` when
  access is denied.
  """
  @spec authorize_list(map() | nil) ::
          {:ok, {String.t(), [term()]}} | {:error, String.t()}
  def authorize_list(user) do
    cond do
      superuser?(user) ->
        {:ok, {"", []}}

      true ->
        rule = rules()["listRule"]

        case Enforcer.classify(rule) do
          :public -> {:ok, {"", []}}
          :filter -> compile(rule, user)
          _ -> {:error, "Access denied by file listRule"}
        end
    end
  end

  @doc """
  Authorizes deleting a specific file record.
  """
  @spec authorize_delete(map() | nil, map()) :: :ok | {:error, String.t()}
  def authorize_delete(user, file_record) do
    if superuser?(user) do
      :ok
    else
      rule = rules()["deleteRule"]

      case Enforcer.classify(rule) do
        :public ->
          :ok

        :filter ->
          if matches_record?(rule, user, file_record) do
            :ok
          else
            {:error, "Access denied by file deleteRule"}
          end

        _ ->
          {:error, "Access denied by file deleteRule"}
      end
    end
  end

  @doc """
  True when `field` is a valid column in a file rule (for editor validation).
  """
  @spec valid_field?(String.t()) :: boolean()
  def valid_field?(field) when is_binary(field), do: Map.has_key?(@field_types, field)
  def valid_field?(_), do: false

  # ── Private ──────────────────────────────────────────

  defp compile(rule, user) do
    Enforcer.compile_for(rule, user, @field_types, "_files")
  end

  # Mirrors `Enforcer.eval_against_context/4` for a `_files` row: bind the row
  # id as $1 (shifting the rule's placeholders up by one) and evaluate the rule
  # against the physical row. Any failure denies rather than crashing.
  defp matches_record?(rule, user, file_record) do
    with {:ok, {sql, params}} <- compile(rule, user),
         true <- sql != "",
         {:ok, id_value} <- TypeMapper.coerce_value("UUID", file_record["id"]) do
      rule_sql = FilterCompiler.shift_placeholders(sql, 1)

      case Ecto.Adapters.SQL.query(
             Repo,
             ~s[SELECT 1 FROM "_files" WHERE id = $1::UUID AND (#{rule_sql}) LIMIT 1],
             [id_value | params]
           ) do
        {:ok, %{rows: rows}} -> length(rows) > 0
        {:error, _} -> false
      end
    else
      _ -> false
    end
  end
end
