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
  """

  use LazypockWeb, :controller

  alias Lazypock.Collections.Registry
  alias Lazypock.Schemas.GenericRecord
  alias Lazypock.Schemas.FilterCompiler
  alias LazypockWeb.DynamicView

  # ── List (GET /api/:collection) ─────────────────────

  def list(conn, %{"collection" => name} = params) do
    with {:ok, collection} <- Registry.get(name) do
      page = max(1, String.to_integer(params["page"] || "1"))
      per_page = max(1, min(200, String.to_integer(params["perPage"] || "30")))
      offset = (page - 1) * per_page

      # Build filtered query
      {where_clause, where_params} = build_filter(params["filter"])
      order_clause = build_sort(params["sort"], collection)

      # Count total (for pagination)
      total =
        if params["skipTotal"] == "true" do
          0
        else
          GenericRecord.count_where(name, where_clause, where_params)
        end

      # Fetch records
      records =
        GenericRecord.all_where(
          name,
          where_clause <>
            " " <>
            order_clause <>
            " LIMIT $#{length(where_params) + 1} OFFSET $#{length(where_params) + 2}",
          where_params ++ [per_page, offset]
        )

      items = DynamicView.format_items(records, name)

      conn
      |> put_resp_header("x-total-count", to_string(total))
      |> json(DynamicView.paginated_response(items, total, page, per_page))
    end
  end

  # ── Show (GET /api/:collection/:id) ─────────────────

  def show(conn, %{"collection" => name, "id" => id}) do
    with {:ok, _collection} <- Registry.get(name) do
      record = GenericRecord.get(name, id)

      if record do
        conn |> json(DynamicView.format_item(record, name))
      else
        conn
        |> put_status(404)
        |> json(error_response(404, "The requested resource wasn't found."))
      end
    end
  end

  # ── Create (POST /api/:collection) ──────────────────

  def create(conn, %{"collection" => name, "data" => attrs}) do
    with {:ok, _collection} <- Registry.get(name) do
      case GenericRecord.insert(name, attrs) do
        {:ok, record} ->
          conn
          |> put_status(201)
          |> json(DynamicView.format_item(record, name))

        {:error, reason} ->
          conn
          |> put_status(400)
          |> json(error_response(400, reason))
      end
    end
  end

  # ── Update (PATCH /api/:collection/:id) ─────────────

  def update(conn, %{"collection" => name, "id" => id} = params) do
    with {:ok, _collection} <- Registry.get(name) do
      attrs = params["data"] || params

      case GenericRecord.update(name, id, attrs) do
        nil ->
          conn
          |> put_status(404)
          |> json(error_response(404, "The requested resource wasn't found."))

        record ->
          conn |> json(DynamicView.format_item(record, name))
      end
    end
  end

  # ── Delete (DELETE /api/:collection/:id) ────────────

  def delete(conn, %{"collection" => name, "id" => id}) do
    with {:ok, _collection} <- Registry.get(name) do
      case GenericRecord.delete(name, id) do
        :ok ->
          conn |> put_status(204) |> json(nil)

        {:error, reason} ->
          conn
          |> put_status(400)
          |> json(error_response(400, reason))
      end
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

  defp error_response(code, message) do
    %{"code" => code, "message" => message, "data" => %{}}
  end
end
