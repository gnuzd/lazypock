defmodule Lazypock.Schema.ViewBuilder do
  @moduledoc """
  Generates the SQL for a "view builder" collection from a structured,
  no-code spec, so end users can create/update read-only views without
  writing SQL themselves.

  The spec is meant to live in the collection's `options["view_builder"]`,
  while the generated SQL is stored in `options["view_query"]` — the single
  source of truth the DDL engine still introspects and validates
  (`Lazypock.Schema.Views`). `options["view_origin"]` records how a view was
  authored (`"builder"` or `"sql"`) so the Studio can keep hand-written views
  in the SQL editor. This module is pure: it only builds and renders SQL, it
  never touches the database.

  ## Spec

      %{
        "source" => "orders",
        "relations" => [%{"alias" => "t1", "field" => "customer"}],
        "fields" => [
          %{"source" => "orders", "name" => "id"},
          %{"source" => "orders", "name" => "is_active"},
          %{"source" => "t1", "name" => "name", "as" => "customer_name"}
        ],
        "sort" => "-created_at",
        "limit" => 100
      }

    * `source`    — required base collection name.
    * `relations` — one entry per relation field whose target columns are
      pulled in. Only `alias` and `field` are needed: the target collection
      and the single/multi multiplicity are derived server-side from the
      field metadata.
    * `fields`    — selected columns. `source` is the base collection name
      (or omitted) or a declared relation alias. `as` is the output column
      name; it defaults to the field name for base columns and to
      `<relation_field>_<name>` for related columns. The unique `id` column
      is injected automatically when absent. Output names are de-duplicated
      with a numeric suffix so a collision can never produce invalid SQL.
    * `sort`      — optional PocketBase-style sort over **base** columns
      (comma separated, `-` prefix = descending, `+` = ascending).
    * `limit`     — optional positive integer.

  ## Joins

    * A **single** relation becomes
      `LEFT JOIN <target> AS <alias> ON <alias>."id"::text = <source>.<field>`.
      At most one target row matches, so the base `id` stays unique — a hard
      requirement for view collections.
    * A **multi** relation becomes a correlated `jsonb_agg` subquery that
      returns an array, mirroring PocketBase's array `expand` semantics. A
      plain JOIN would fan the base row out and break the unique `id`.

  Every identifier is validated against the collection registry and emitted
  through `TypeMapper.quote_ident/1`, so casing (`isActive` vs `is_active`)
  is preserved verbatim.
  """

  alias Lazypock.Collections.Collection
  alias Lazypock.Collections.Registry
  alias Lazypock.Schema.TypeMapper

  @identifier ~r/^[A-Za-z_][A-Za-z0-9_]*$/

  @typedoc "A no-code view builder spec (string keys, as decoded from JSON)."
  @type spec :: map()

  @doc """
  Generates SQL from `spec`, resolving collections from the live registry.
  """
  @spec to_query(spec()) :: {:ok, String.t()} | {:error, String.t()}
  def to_query(spec) when is_map(spec), do: to_query(spec, Registry.list())

  @doc """
  Generates SQL from `spec` against an explicit list of collections.

  Exposed mainly for tests and callers that already hold a registry snapshot.
  """
  @spec to_query(spec(), [Collection.t()]) :: {:ok, String.t()} | {:error, String.t()}
  def to_query(spec, collections) when is_map(spec) and is_list(collections) do
    with {:ok, plan} <- build_plan(spec, collections) do
      {:ok, render(plan)}
    end
  end

  # ── Plan building (validation + normalization) ──────

  defp build_plan(spec, collections) do
    by_name = Map.new(collections, fn collection -> {collection.name, collection} end)

    with {:ok, source} <- fetch_collection(by_name, spec["source"], "source collection"),
         {:ok, relations} <- build_relations(spec["relations"] || [], source, by_name),
         {:ok, fields} <- build_fields(spec["fields"] || [], source, relations),
         {:ok, fields} <- ensure_id(fields),
         fields <- uniquify_aliases(fields),
         {:ok, sort} <- build_sort(spec["sort"], source),
         {:ok, limit} <- build_limit(spec["limit"]) do
      {:ok, %{source: source, relations: relations, fields: fields, sort: sort, limit: limit}}
    end
  end

  defp fetch_collection(by_name, name, label) do
    cond do
      not is_binary(name) or name == "" -> {:error, "#{label} is required"}
      Map.has_key?(by_name, name) -> {:ok, Map.fetch!(by_name, name)}
      true -> {:error, "#{label} '#{name}' does not exist"}
    end
  end

  # ── Relations ───────────────────────────────────────

  defp build_relations(relations, source, by_name) when is_list(relations) do
    relations
    |> Enum.reduce_while({:ok, %{}}, fn relation, {:ok, acc} ->
      with {:ok, alias_name} <- validate_identifier(relation["alias"], "relation alias"),
           :ok <- reject_source_alias(alias_name, source),
           :ok <- reject_duplicate_alias(acc, alias_name),
           {:ok, base_field} <- fetch_relation_field(source, relation["field"]),
           {:ok, target_name} <- relation_target_name(base_field),
           {:ok, target} <- fetch_collection(by_name, target_name, "relation target") do
        entry = %{
          alias: alias_name,
          field: base_field.name,
          target: target,
          multi: multi_relation?(base_field)
        }

        {:cont, {:ok, Map.put(acc, alias_name, entry)}}
      else
        {:error, reason} -> {:halt, {:error, reason}}
      end
    end)
  end

  defp build_relations(_other, _source, _by_name), do: {:error, "relations must be a list"}

  defp fetch_relation_field(source, name) do
    case field_by_name(source, name) do
      %{type: "relation"} = field ->
        {:ok, field}

      nil ->
        {:error, "relation field '#{inspect(name)}' does not exist on '#{source.name}'"}

      %{name: field_name} ->
        {:error, "field '#{field_name}' on '#{source.name}' is not a relation"}
    end
  end

  defp relation_target_name(field) do
    case field.options["collection"] do
      name when is_binary(name) and name != "" -> {:ok, name}
      _ -> {:error, "relation field '#{field.name}' has no target collection"}
    end
  end

  defp multi_relation?(field) do
    case field.options["maxSelect"] do
      max when is_integer(max) and max > 1 -> true
      _ -> false
    end
  end

  defp reject_source_alias(alias_name, source) do
    if alias_name == source.name do
      {:error, "relation alias '#{alias_name}' conflicts with the source collection name"}
    else
      :ok
    end
  end

  defp reject_duplicate_alias(acc, alias_name) do
    if Map.has_key?(acc, alias_name) do
      {:error, "duplicate relation alias '#{alias_name}'"}
    else
      :ok
    end
  end

  # ── Fields ──────────────────────────────────────────

  defp build_fields(fields, source, relations) when is_list(fields) do
    fields
    |> Enum.reduce_while({:ok, []}, fn field, {:ok, acc} ->
      case build_field(field, source, relations) do
        {:ok, built} -> {:cont, {:ok, [built | acc]}}
        {:error, reason} -> {:halt, {:error, reason}}
      end
    end)
    |> case do
      {:ok, built} -> {:ok, Enum.reverse(built)}
      {:error, _reason} = error -> error
    end
  end

  defp build_fields(_other, _source, _relations), do: {:error, "fields must be a list"}

  defp build_field(field, source, relations) do
    field_source = field["source"] || source.name
    name = field["name"]

    cond do
      field_source == source.name ->
        with {:ok, column} <- fetch_column(source, name, "field"),
             {:ok, as} <- output_alias(field["as"], name) do
          {:ok, %{kind: :base, name: column.name, as: as}}
        end

      Map.has_key?(relations, field_source) ->
        relation = Map.fetch!(relations, field_source)

        with {:ok, column} <- fetch_column(relation.target, name, "related field"),
             {:ok, as} <- output_alias(field["as"], "#{relation.field}_#{name}") do
          {:ok, %{kind: :related, relation: relation, name: column.name, as: as}}
        end

      true ->
        {:error, "unknown field source '#{field_source}'"}
    end
  end

  defp fetch_column(_collection, "id", _label), do: {:ok, %{name: "id", type: "id"}}

  defp fetch_column(collection, name, label) when is_binary(name) do
    case field_by_name(collection, name) do
      nil ->
        {:error, "#{label} '#{name}' does not exist on '#{collection.name}'"}

      %{hidden: true} ->
        {:error, "#{label} '#{name}' is hidden"}

      %{type: "password"} ->
        {:error, "#{label} '#{name}' is a password field"}

      field ->
        {:ok, field}
    end
  end

  defp fetch_column(collection, _name, label) do
    {:error, "#{label} name is required on '#{collection.name}'"}
  end

  defp ensure_id(fields) do
    base_id = %{kind: :base, name: "id", as: "id"}

    if Enum.member?(fields, base_id) do
      {:ok, fields}
    else
      {:ok, [base_id | fields]}
    end
  end

  # Deterministic de-duplication: `name`, `name_2`, `name_3`, … The preview
  # endpoint returns the resulting names so the UI can show exactly what will
  # be created.
  defp uniquify_aliases(fields) do
    {fields, _used} =
      Enum.map_reduce(fields, MapSet.new(), fn field, used ->
        name = unique_name(field.as, used)
        {Map.put(field, :as, name), MapSet.put(used, name)}
      end)

    fields
  end

  defp unique_name(name, used) do
    if MapSet.member?(used, name) do
      Stream.iterate(2, &(&1 + 1))
      |> Enum.find_value(fn suffix ->
        candidate = "#{name}_#{suffix}"
        if MapSet.member?(used, candidate), do: nil, else: candidate
      end)
    else
      name
    end
  end

  defp field_by_name(collection, name) when is_binary(name) do
    Enum.find(collection.fields || [], fn field -> field.name == name end)
  end

  defp field_by_name(_collection, _name), do: nil

  defp output_alias(as, _default) when is_binary(as) and as != "" do
    validate_identifier(as, "output column name")
  end

  defp output_alias(as, _default) when not is_nil(as) and as != "" do
    {:error, "output column name must be a string"}
  end

  defp output_alias(_as, default), do: validate_identifier(default, "output column name")

  # ── Sort / limit ────────────────────────────────────

  defp build_sort(nil, _source), do: {:ok, []}
  defp build_sort("", _source), do: {:ok, []}

  defp build_sort(sort, source) when is_binary(sort) do
    sort
    |> String.split(",", trim: true)
    |> Enum.map(&String.trim/1)
    |> Enum.reject(&(&1 == ""))
    |> Enum.reduce_while({:ok, []}, fn token, {:ok, acc} ->
      {direction, name} =
        case token do
          "-" <> rest -> {:desc, rest}
          "+" <> rest -> {:asc, rest}
          rest -> {:asc, rest}
        end

      case fetch_column(source, String.trim(name), "sort field") do
        {:ok, column} -> {:cont, {:ok, [{direction, column.name} | acc]}}
        {:error, reason} -> {:halt, {:error, reason}}
      end
    end)
    |> case do
      {:ok, sort} -> {:ok, Enum.reverse(sort)}
      {:error, _reason} = error -> error
    end
  end

  defp build_sort(_sort, _source), do: {:error, "sort must be a string"}

  defp build_limit(nil), do: {:ok, nil}
  defp build_limit(limit) when is_integer(limit) and limit > 0, do: {:ok, limit}
  defp build_limit(_limit), do: {:error, "limit must be a positive integer"}

  # ── Rendering ───────────────────────────────────────

  defp render(plan) do
    source = plan.source.name

    select =
      plan.fields
      |> Enum.map(fn field ->
        "    " <> select_expr(field, source) <> " AS " <> quote_ident(field.as)
      end)
      |> Enum.join(",\n")

    joins =
      plan.relations
      |> Map.values()
      |> Enum.reject(& &1.multi)
      |> Enum.map(&join_expr(&1, source))

    ["SELECT", select, "FROM #{quote_ident(source)}" | joins]
    |> Kernel.++(sort_lines(plan.sort, source))
    |> Kernel.++(limit_lines(plan.limit))
    |> Enum.join("\n")
  end

  defp select_expr(%{kind: :base, name: name}, source) do
    "#{quote_ident(source)}.#{quote_ident(name)}"
  end

  defp select_expr(%{kind: :related, relation: %{multi: false} = relation, name: name}, _source) do
    "#{quote_ident(relation.alias)}.#{quote_ident(name)}"
  end

  defp select_expr(%{kind: :related, relation: relation, name: name}, source) do
    inner = "_agg"
    column = "#{quote_ident(inner)}.#{quote_ident(name)}"
    inner_id = "#{quote_ident(inner)}.\"id\""

    base_relation =
      "#{quote_ident(source)}.#{quote_ident(relation.field)}"

    "(SELECT COALESCE(jsonb_agg(#{column} ORDER BY #{inner_id}), '[]'::jsonb) " <>
      "FROM #{quote_ident(relation.target.name)} AS #{quote_ident(inner)} " <>
      "WHERE #{inner_id}::text = ANY(#{base_relation}))"
  end

  defp join_expr(relation, source) do
    alias_name = quote_ident(relation.alias)

    "LEFT JOIN #{quote_ident(relation.target.name)} AS #{alias_name} " <>
      "ON #{alias_name}.\"id\"::text = #{quote_ident(source)}.#{quote_ident(relation.field)}"
  end

  defp sort_lines([], _source), do: []

  defp sort_lines(sort, source) do
    body =
      sort
      |> Enum.map(fn {direction, name} ->
        "#{quote_ident(source)}.#{quote_ident(name)} #{direction_sql(direction)}"
      end)
      |> Enum.join(", ")

    ["ORDER BY " <> body]
  end

  defp direction_sql(:desc), do: "DESC"
  defp direction_sql(:asc), do: "ASC"

  defp limit_lines(nil), do: []
  defp limit_lines(limit), do: ["LIMIT #{limit}"]

  # ── Small helpers ───────────────────────────────────

  defp validate_identifier(value, label) when is_binary(value) do
    if Regex.match?(@identifier, value) do
      {:ok, value}
    else
      {:error,
       "#{label} '#{value}' must start with a letter or underscore and contain only letters, digits, and underscores"}
    end
  end

  defp validate_identifier(_value, label), do: {:error, "#{label} is required"}

  defp quote_ident(name), do: TypeMapper.quote_ident(name)
end
