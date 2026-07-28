defmodule LazypockWeb.DynamicController do
  @moduledoc """
  Generic controller that handles CRUD operations for any dynamic collection.

  All routes are in the format `/api/:collection` and `/api/:collection/:id`.
  The controller looks up the collection in the Registry, validates access
  (Phase 4), and delegates to GenericRecord for the actual database operations.

  ## Query parameters (list action only)

    * `filter` — PocketBase filter syntax: `title~'hello' && published=true`
    * `sort` — Comma-separated, `-` prefix for DESC: `-created,title`
    * `page` — Page number (default: 1)
    * `perPage` — Items per page (default: 30, max: 200)
    * `fields` — Comma-separated field names to return
    * `skipTotal` — If `true`, skip total count query (performance)
    * `expand` — Comma-separated relation field names to expand: `author,category`

  ## Query parameters (show action)

    * `expand` — Comma-separated relation field names to expand: `author,category`
  """

  use LazypockWeb, :controller

  alias Lazypock.Collections.Registry
  alias Lazypock.Schemas.GenericRecord
  alias Lazypock.Schemas.FilterCompiler
  alias LazypockWeb.DynamicView
  alias Lazypock.Rules.Enforcer
  alias Lazypock.Realtime.Broadcaster
  alias Lazypock.Files.Store
  alias Lazypock.Hooks.Dispatcher, as: Hooks

  # ── List (GET /api/:collection) ─────────────────────

  def list(conn, %{"collection" => name} = params) do
    user = resolve_user(conn)

    with {:ok, collection} <- Registry.get(name),
         {:ok, {rule_where, rule_params}} <- Enforcer.authorize_list(name, user) do
      page = max(1, String.to_integer(params["page"] || "1"))
      per_page = max(1, min(200, String.to_integer(params["perPage"] || "30")))
      offset = (page - 1) * per_page

      # Build filtered query — merge rule WHERE with user filter WHERE
      {filter_where, filter_params} = build_filter(params["filter"])
      order_clause = build_sort(params["sort"], collection)

      # Combine WHERE clauses: user_filter AND rule
      combined_where = combine_wheres(filter_where, rule_where)
      combined_params = filter_params ++ rule_params

      total =
        if params["skipTotal"] == "true" do
          0
        else
          GenericRecord.count_where(name, combined_where, combined_params)
        end

      records =
        GenericRecord.all_where(
          name,
          combined_where <>
            " " <>
            order_clause <>
            " LIMIT $#{length(combined_params) + 1} OFFSET $#{length(combined_params) + 2}",
          combined_params ++ [per_page, offset]
        )

      items = DynamicView.format_items(records, name)
      items = DynamicView.expand_records(items, params["expand"], name)

      conn
      |> put_resp_header("x-total-count", to_string(total))
      |> json(DynamicView.paginated_response(items, total, page, per_page))
    else
      {:error, :not_found} ->
        conn
        |> put_status(404)
        |> json(error_response(404, "Collection not found"))

      {:error, reason} ->
        conn
        |> put_status(403)
        |> json(error_response(403, reason))
    end
  end

  # ── Show (GET /api/:collection/:id) ─────────────────

  def show(conn, %{"collection" => name, "id" => id} = params) do
    user = resolve_user(conn)

    with {:ok, _collection} <- Registry.get(name),
         record when not is_nil(record) <- GenericRecord.get(name, id),
         :ok <- Enforcer.authorize_view(name, user, record) do
      item = DynamicView.format_item(record, name)
      [item] = DynamicView.expand_records([item], params["expand"], name)
      conn |> json(item)
    else
      nil ->
        conn
        |> put_status(404)
        |> json(error_response(404, "The requested resource wasn't found."))

      {:error, :not_found} ->
        conn
        |> put_status(404)
        |> json(error_response(404, "Collection not found"))

      {:error, reason} ->
        conn
        |> put_status(403)
        |> json(error_response(403, reason))
    end
  end

  # ── Create (POST /api/:collection) ──────────────────

  def create(conn, %{"collection" => name} = params) do
    user = resolve_user(conn)
    context = %{collection_name: name, user: user, conn: conn}

    with {:ok, collection} <- Registry.get(name),
         raw_attrs = Map.get(params, "data") || Map.drop(params, ["collection"]),
         attrs = sanitize_attrs(raw_attrs, collection),
         :ok <- Enforcer.authorize_create(name, user, attrs),
         {:ok, enriched_attrs} <- Hooks.dispatch_create(attrs, context) do
      case GenericRecord.insert(name, Map.drop(enriched_attrs, ["id"])) do
        {:ok, record} ->
          Broadcaster.broadcast_create(name, record)
          Hooks.dispatch_after_create(record, context)

          conn
          |> put_status(201)
          |> json(DynamicView.format_item(record, name))

        {:error, reason} ->
          conn
          |> put_status(400)
          |> json(error_response(400, reason))
      end
    else
      {:error, :not_found} ->
        conn
        |> put_status(404)
        |> json(error_response(404, "Collection not found"))

      {:error, reason} ->
        conn
        |> put_status(403)
        |> json(error_response(403, reason))
    end
  end

  # ── Update (PATCH /api/:collection/:id) ─────────────

  def update(conn, %{"collection" => name, "id" => id} = params) do
    user = resolve_user(conn)
    context = %{collection_name: name, user: user, conn: conn}

    with {:ok, collection} <- Registry.get(name),
         record when not is_nil(record) <- GenericRecord.get(name, id),
         :ok <- Enforcer.authorize_update(name, user, record),
         raw_attrs = params["data"] || params,
         attrs = sanitize_attrs(raw_attrs, collection),
         {:ok, enriched_attrs} <- Hooks.dispatch_update(record, attrs, context),
         updated_record when not is_nil(updated_record) <-
           GenericRecord.update(
             name,
             id,
             Map.drop(enriched_attrs, ["id", "created_at", "updated_at", "collectionName"])
           ) do
      Broadcaster.broadcast_update(name, updated_record)
      conn |> json(DynamicView.format_item(updated_record, name))
    else
      nil ->
        conn
        |> put_status(404)
        |> json(error_response(404, "The requested resource wasn't found."))

      {:error, :not_found} ->
        conn
        |> put_status(404)
        |> json(error_response(404, "Collection not found"))

      {:error, reason} ->
        conn
        |> put_status(403)
        |> json(error_response(403, reason))
    end
  end

  # ── Delete (DELETE /api/:collection/:id) ────────────

  def delete(conn, %{"collection" => name, "id" => id}) do
    user = resolve_user(conn)
    context = %{collection_name: name, user: user, conn: conn}

    with {:ok, _collection} <- Registry.get(name),
         record when not is_nil(record) <- GenericRecord.get(name, id),
         :ok <- Enforcer.authorize_delete(name, user, record),
         :ok <- Hooks.dispatch_delete(record, context),
         :ok <- GenericRecord.delete(name, id) do
      Store.delete_by_record(name, id)
      Broadcaster.broadcast_delete(name, id)
      Hooks.dispatch_after_delete(record, context)
      conn |> put_status(204) |> json(nil)
    else
      nil ->
        conn
        |> put_status(404)
        |> json(error_response(404, "The requested resource wasn't found."))

      {:error, :not_found} ->
        conn
        |> put_status(404)
        |> json(error_response(404, "Collection not found"))

      {:error, reason} ->
        conn
        |> put_status(403)
        |> json(error_response(403, reason))
    end
  end

  # ── Private helpers ─────────────────────────────────

  defp build_filter(nil), do: {"", []}

  defp build_filter(filter_str) when is_binary(filter_str) do
    case FilterCompiler.compile(filter_str) do
      {:ok, {sql_clause, params}} -> {sql_clause, params}
      {:error, _} -> {"", []}
    end
  end

  defp build_sort(nil, _collection), do: "ORDER BY \"created_at\" DESC"

  defp build_sort(sort_str, _collection) when is_binary(sort_str) do
    clauses =
      sort_str
      |> String.split(",")
      |> Enum.map(&String.trim/1)
      |> Enum.map(fn
        "-" <> field -> ~s["#{field}" DESC]
        field -> ~s["#{field}" ASC]
      end)
      |> Enum.join(", ")

    "ORDER BY #{clauses}"
  end

  # Return the authenticated user for Enforcer rule resolution.
  # Superusers pass their struct (triggering superuser bypass).
  # Auth collection users pass their record map (for @request.auth.*).
  defp resolve_user(conn) do
    conn.assigns[:current_superuser] || conn.assigns[:current_user]
  end

  defp error_response(code, message) do
    %{"code" => code, "message" => message, "data" => %{}}
  end

  defp combine_wheres("", ""), do: ""
  defp combine_wheres("", sql), do: sql
  defp combine_wheres(sql, ""), do: sql
  defp combine_wheres(left, right), do: "(#{left}) AND (#{right})"

  # Strip system fields, coerce empty strings to nil for non-text types,
  # and bcrypt-hash password fields automatically.
  # Empty password values are dropped entirely (keep existing on update).
  defp sanitize_attrs(attrs, collection) do
    field_types =
      (collection.fields || [])
      |> Enum.map(fn f -> {f.name, f.type} end)
      |> Map.new()

    non_text_types = MapSet.new(["number", "bool", "date", "datetime"])
    system_fields = MapSet.new(["id", "created_at", "updated_at", "collectionName", "collection"])

    attrs
    |> Map.drop(MapSet.to_list(system_fields))
    |> Enum.reduce(%{}, fn {key, value}, acc ->
      type = Map.get(field_types, key, "text")

      cond do
        # Empty password on update → skip entirely (keep existing)
        type == "password" and value == "" ->
          acc

        # Password with value → bcrypt hash
        type == "password" and is_binary(value) ->
          Map.put(acc, key, Bcrypt.hash_pwd_salt(value))

        # Empty string for non-text type → nil
        value == "" and MapSet.member?(non_text_types, type) ->
          Map.put(acc, key, nil)

        # Everything else as-is
        true ->
          Map.put(acc, key, value)
      end
    end)
  end
end
