defmodule LazypockWeb.CollectionController do
  use LazypockWeb, :controller

  alias Lazypock.Collections.Registry, as: CollectionRegistry
  alias Lazypock.Schema.DDL
  alias Lazypock.Realtime.Broadcaster
  alias Lazypock.Rules.Enforcer

  # ── Superuser guard ────────────────────────────────
  # list + create operate outside any single collection context,
  # so only superusers can enumerate or create collections.
  # show / update / delete use Enforcer.authorize_manage to
  # honor manageRule for delegated collection management.

  defp require_superuser!(conn) do
    case conn.assigns[:current_superuser] do
      nil ->
        conn
        |> put_resp_content_type("application/json")
        |> send_resp(
          403,
          Jason.encode!(%{code: 403, message: "Access denied. Superuser required.", data: %{}})
        )
        |> halt()

      _user ->
        conn
    end
  end

  # List all collections
  def list(conn, _params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_list(conn)
  end

  defp do_list(conn) do
    collections = CollectionRegistry.list()
    json(conn, %{items: Enum.map(collections, &collection_json/1)})
  end

  # Create a new collection
  def create(conn, %{"name" => name} = params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_create(conn, name, params)
  end

  defp do_create(conn, name, params) do
    type = params["type"] || "base"
    fields = params["fields"] || []

    case DDL.create_collection(name, type: type, fields: fields) do
      {:ok, collection} ->
        Broadcaster.broadcast_collection_event("create", collection_json(collection))
        conn |> put_status(201) |> json(collection_json(collection))

      {:error, reason} ->
        conn |> put_status(400) |> json(%{error: reason})
    end
  end

  # Get a single collection
  def show(conn, %{"id" => name_or_id}) do
    user = conn.assigns[:current_superuser]
    do_show(conn, name_or_id, user)
  end

  defp do_show(conn, name_or_id, user) do
    case CollectionRegistry.get(name_or_id) do
      {:ok, collection} ->
        case Enforcer.authorize_manage(collection.name, user) do
          :ok ->
            json(conn, collection_json(collection))

          {:error, reason} ->
            conn |> put_status(403) |> json(%{error: reason})
        end

      {:error, :not_found} ->
        # Fallback: try lookup by UUID across all collections
        case Enum.find(CollectionRegistry.list(), &(&1.id == name_or_id)) do
          nil ->
            conn |> put_status(404) |> json(%{error: "Collection not found"})

          collection ->
            case Enforcer.authorize_manage(collection.name, user) do
              :ok ->
                json(conn, collection_json(collection))

              {:error, reason} ->
                conn |> put_status(403) |> json(%{error: reason})
            end
        end
    end
  end

  # Update a collection
  def update(conn, %{"id" => id} = params) do
    user = conn.assigns[:current_superuser]
    do_update(conn, id, params, user)
  end

  defp do_update(conn, id, params, user) do
    coll_name = resolve_collection_name(id)

    cond do
      is_nil(coll_name) ->
        conn |> put_status(404) |> json(%{error: "Collection not found"})

      true ->
        case Enforcer.authorize_manage(coll_name, user) do
          {:error, reason} ->
            conn |> put_status(403) |> json(%{error: reason})

          :ok ->
            # Build rules map from flat rule keys if present (SPA sends them flat)
            rules =
              if params["rules"] do
                # Strip nil values from the rules map, treat as superuser-only
                params["rules"]
                |> Enum.filter(fn {_k, v} -> not is_nil(v) end)
                |> Map.new()
              else
                rule_keys = [
                  "listRule",
                  "viewRule",
                  "createRule",
                  "updateRule",
                  "deleteRule",
                  "manageRule"
                ]

                rule_keys
                |> Enum.reduce(%{}, fn key, acc ->
                  case Map.fetch(params, key) do
                    {:ok, value} when not is_nil(value) -> Map.put(acc, key, value)
                    _ -> acc
                  end
                end)
              # Always pass even if empty — empty map clears old rules to superuser-only
              end

            opts =
              []
              |> maybe_put(:name, params["name"])
              |> maybe_put(:type, params["type"])
              |> maybe_put(:fields, params["fields"])
              |> maybe_put(:rules, rules)
              |> maybe_put(:options, params["options"])
              |> maybe_put(:hooks, params["hooks"])

            case DDL.update_collection(coll_name, opts) do
              {:ok, collection} ->
                Broadcaster.broadcast_collection_event("update", collection_json(collection))
                json(conn, collection_json(collection))

              {:error, reason} ->
                conn |> put_status(400) |> json(%{error: reason})
            end
        end
    end
  end

  # Delete a collection
  def delete(conn, %{"id" => id}) do
    user = conn.assigns[:current_superuser]
    do_delete(conn, id, user)
  end

  defp do_delete(conn, id, user) do
    coll_name = resolve_collection_name(id)

    if is_nil(coll_name) do
      conn |> put_status(404) |> json(%{error: "Collection not found"})
    else
      case Enforcer.authorize_manage(coll_name, user) do
        {:error, reason} ->
          conn |> put_status(403) |> json(%{error: reason})

        :ok ->
          case DDL.drop_collection(coll_name) do
            :ok ->
              Broadcaster.broadcast_collection_event("delete", %{id: id})
              conn |> put_status(204) |> json(%{ok: true})

            {:error, reason} ->
              conn |> put_status(400) |> json(%{error: reason})
          end
      end
    end
  end

  defp collection_json(collection) do
    %{
      id: collection.id,
      name: collection.name,
      type: collection.type,
      system: Map.get(collection, :system, false),
      schema: collection.schema,
      fields:
        (collection.fields || [])
        |> Enum.sort_by(& &1.sort_order, :asc)
        |> Enum.map(fn f ->
          %{
            id: f.id,
            name: f.name,
            type: f.type,
            required: f.required,
            unique: f.unique,
            options: f.options,
            indexed: f.indexed,
            hidden: f.hidden,
            system: f.system,
            sort_order: f.sort_order
          }
        end),
      rules: collection.rules,
      options: collection.options,
      created: collection.created_at,
      updated: collection.updated_at
    }
  end

  defp maybe_put(opts, _key, nil), do: opts
  defp maybe_put(opts, key, value), do: Keyword.put(opts, key, value)

  # Resolve a collection name from either a UUID or a name string
  defp resolve_collection_name(id_or_name) do
    case CollectionRegistry.get(id_or_name) do
      {:ok, collection} ->
        collection.name

      {:error, :not_found} ->
        # Fallback: try lookup by UUID across all collections
        case Enum.find(CollectionRegistry.list(), &(&1.id == id_or_name)) do
          nil -> nil
          collection -> collection.name
        end
    end
  end
end
