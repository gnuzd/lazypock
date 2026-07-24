defmodule LazypockWeb.CollectionController do
  use LazypockWeb, :controller

  alias Lazypock.Collections.Registry, as: CollectionRegistry
  alias Lazypock.Schema.DDL
  alias Lazypock.Realtime.Broadcaster

  # List all collections
  def list(conn, _params) do
    collections = CollectionRegistry.list()
    json(conn, %{items: Enum.map(collections, &collection_json/1)})
  end

  # Create a new collection
  def create(conn, %{"name" => name} = params) do
    type = params["type"] || "base"
    fields = params["fields"] || []

    case DDL.create_collection(name, type: type, fields: fields) do
      {:ok, collection} ->
        Broadcaster.broadcast_collection_event("create", collection_json(collection))

        conn
        |> put_status(201)
        |> json(collection_json(collection))

      {:error, reason} ->
        conn
        |> put_status(400)
        |> json(%{error: reason})
    end
  end

  # Get a single collection
  def show(conn, %{"id" => name_or_id}) do
    case CollectionRegistry.get(name_or_id) do
      {:ok, collection} ->
        json(conn, collection_json(collection))

      {:error, :not_found} ->
        # Fallback: try lookup by UUID across all collections
        case Enum.find(CollectionRegistry.list(), &(&1.id == name_or_id)) do
          nil -> conn |> put_status(404) |> json(%{error: "Collection not found"})
          collection -> json(conn, collection_json(collection))
        end
    end
  end

  # Update a collection
  def update(conn, %{"id" => id} = params) do
    coll_name = resolve_collection_name(id)

    cond do
      is_nil(coll_name) ->
        conn |> put_status(404) |> json(%{error: "Collection not found"})

      true ->
        opts =
          []
          |> maybe_put(:name, params["name"])
          |> maybe_put(:type, params["type"])
          |> maybe_put(:fields, params["fields"])

        case DDL.update_collection(coll_name, opts) do
          {:ok, collection} ->
            Broadcaster.broadcast_collection_event("update", collection_json(collection))
            json(conn, collection_json(collection))

          {:error, reason} ->
            conn |> put_status(400) |> json(%{error: reason})
        end
    end
  end

  # Delete a collection
  def delete(conn, %{"id" => id}) do
    coll_name = resolve_collection_name(id)

    if is_nil(coll_name) do
      conn |> put_status(404) |> json(%{error: "Collection not found"})
    else
      case DDL.drop_collection(coll_name) do
        :ok ->
          Broadcaster.broadcast_collection_event("delete", %{id: id})
          conn |> put_status(204) |> json(%{ok: true})

        {:error, reason} ->
          conn |> put_status(400) |> json(%{error: reason})
      end
    end
  end

  defp collection_json(collection) do
    %{
      id: collection.id,
      name: collection.name,
      type: collection.type,
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
      created: collection.inserted_at,
      updated: collection.updated_at
    }
  end
  defp maybe_put(opts, _key, nil), do: opts
  defp maybe_put(opts, key, value), do: Keyword.put(opts, key, value)

  # Resolve a collection name from either a UUID or a name string
  defp resolve_collection_name(id_or_name) do
    case CollectionRegistry.get(id_or_name) do
      {:ok, collection} -> collection.name
      {:error, :not_found} ->
        # Fallback: try lookup by UUID across all collections
        case Enum.find(CollectionRegistry.list(), &(&1.id == id_or_name)) do
          nil -> nil
          collection -> collection.name
        end
    end
  end
end
