defmodule LazypockWeb.CollectionController do
  use LazypockWeb, :controller

  alias Lazypock.Collections.Registry, as: CollectionRegistry
  alias Lazypock.Schema.DDL
  alias Lazypock.Realtime.Broadcaster
  alias Lazypock.Rules.Enforcer

  # ── Superuser guard ────────────────────────────────
  # list + create operate outside any single collection context,
  # so only superusers can enumerate or create collections.
  # GET /collections additionally accepts a valid API key (codegen uses
  # this to fetch schemas without a login round-trip).
  # show / update / delete use Enforcer.authorize_manage to
  # honor manageRule for delegated collection management.

  # Allow real superusers AND API-key identities (scoped to listing only).
  # API keys are intentionally NOT SuperUser structs, so they cannot
  # bypass rules on other endpoints.
  defp require_superuser_or_api_key!(conn) do
    case {conn.assigns[:current_superuser], conn.assigns[:api_key_identity]} do
      {nil, nil} ->
        conn
        |> put_resp_content_type("application/json")
        |> send_resp(
          403,
          Jason.encode!(%{code: 403, message: "Access denied. Superuser required.", data: %{}})
        )
        |> halt()

      _ ->
        conn
    end
  end

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
    conn = require_superuser_or_api_key!(conn)
    if conn.halted, do: conn, else: do_list(conn)
  end

  defp do_list(conn) do
    collections = CollectionRegistry.list()
    items = Enum.map(collections, &collection_json/1)
    result = %{items: items}

    case Lazypock.Hooks.Request.trigger_collections_list(conn, items, result) do
      {:ok, _event} -> json(conn, result)
      {:error, reason} -> conn |> put_status(400) |> json(%{error: reason})
    end
  end

  # Dry-run a view query (PocketBase parity): validates the query and returns
  # the generated fields plus a sample of up to 10 records.
  # POST /api/collections/meta/dry-run-view
  def dry_run_view(conn, %{"query" => query}) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_dry_run_view(conn, query)
  end

  def dry_run_view(conn, _params) do
    conn = require_superuser!(conn)

    if conn.halted do
      conn
    else
      conn
      |> put_status(400)
      |> json(%{code: 400, message: "Missing query parameter", data: %{}})
    end
  end

  defp do_dry_run_view(conn, query) do
    case Lazypock.Schema.Views.dry_run(query, 10) do
      {:ok, %{fields: fields, sample: sample}} ->
        json(conn, %{fields: fields, sample: sample})

      {:error, reason} ->
        conn
        |> put_status(400)
        |> json(%{
          code: 400,
          message: "Invalid view query. Raw error: \n#{reason}",
          data: %{}
        })
    end
  end

  # Create a new collection
  def create(conn, %{"name" => name} = params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_create(conn, name, params)
  end

  defp do_create(conn, name, params) do
    type = params["type"] || "base"
    fields = params["fields"] || []
    indexes = params["indexes"] || []
    options = build_options(params)

    # Fire onCollectionCreateRequest (PocketBase parity)
    case Lazypock.Hooks.Request.trigger_collection_create(conn, %{name: name, type: type}) do
      {:ok, _event} ->
        case DDL.create_collection(name,
               type: type,
               fields: fields,
               indexes: indexes,
               options: options
             ) do
          {:ok, collection} ->
            Lazypock.Hooks.Collection.trigger_after_create_success(collection)
            Broadcaster.broadcast_collection_event("create", collection_json(collection))
            conn |> put_status(201) |> json(collection_json(collection))

          {:error, reason} ->
            conn |> put_status(400) |> json(%{error: reason})
        end

      {:error, reason} ->
        conn |> put_status(400) |> json(%{error: reason})
    end
  end

  # Builds the collection options map for create/update. Accepts both the
  # PB-style top-level `viewQuery` param and an explicit `options` map;
  # `viewQuery` is stored as `options["view_query"]`. Returns `nil` when
  # neither is present so callers can skip the options update entirely
  # (preserving existing options on partial updates).
  defp build_options(params) do
    case {params["options"], params["viewQuery"]} do
      {nil, nil} -> nil
      {options, nil} -> options
      {options, q} when is_binary(q) -> Map.put(options || %{}, "view_query", q)
      {options, _} -> options
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
            # Build rules map from flat rule keys if present (SPA sends them flat).
            # Only include rules when explicitly provided — omitting rules preserves
            # existing values and prevents silent data loss.
            rule_keys = [
              "listRule",
              "viewRule",
              "createRule",
              "updateRule",
              "deleteRule",
              "manageRule"
            ]

            rules =
              cond do
                is_map(params["rules"]) ->
                  # Strip nil values from the rules map, treat as superuser-only
                  params["rules"]
                  |> Enum.filter(fn {_k, v} -> not is_nil(v) end)
                  |> Map.new()

                Enum.any?(rule_keys, &Map.has_key?(params, &1)) ->
                  rule_keys
                  |> Enum.reduce(%{}, fn key, acc ->
                    case Map.fetch(params, key) do
                      {:ok, value} when not is_nil(value) -> Map.put(acc, key, value)
                      _ -> acc
                    end
                  end)

                true ->
                  # No rule params provided — don't touch existing rules
                  nil
              end

            opts =
              []
              |> maybe_put(:name, params["name"])
              |> maybe_put(:type, params["type"])
              |> maybe_put(:fields, params["fields"])
              |> maybe_put(:rules, rules)
              |> maybe_put(:options, build_options(params))
              |> maybe_put(:hooks, params["hooks"])
              |> maybe_put(:indexes, params["indexes"])

            case Lazypock.Hooks.Request.trigger_collection_update(conn, %{name: coll_name}) do
              {:ok, _event} ->
                case DDL.update_collection(coll_name, opts) do
                  {:ok, collection} ->
                    Lazypock.Hooks.Collection.trigger_after_update_success(collection)
                    Broadcaster.broadcast_collection_event("update", collection_json(collection))
                    json(conn, collection_json(collection))

                  {:error, reason} ->
                    conn |> put_status(400) |> json(%{error: reason})
                end

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
          case Lazypock.Hooks.Request.trigger_collection_delete(conn, %{name: coll_name}) do
            {:ok, _event} ->
              case DDL.drop_collection(coll_name) do
                :ok ->
                  Broadcaster.broadcast_collection_event("delete", %{id: id})
                  conn |> put_status(204) |> json(%{ok: true})

                {:error, reason} ->
                  conn |> put_status(400) |> json(%{error: reason})
              end

            {:error, reason} ->
              conn |> put_status(400) |> json(%{error: reason})
          end
      end
    end
  end

  defp collection_json(collection) do
    opts = collection.options || %{}

    %{
      id: collection.id,
      name: collection.name,
      type: collection.type,
      system: Map.get(collection, :system, false),
      schema: collection.schema,
      indexes: Map.get(opts, "indexes", []) || [],
      viewQuery: if(collection.type == "view", do: Map.get(opts, "view_query"), else: nil),
      fields:
        (collection.fields || [])
        |> Enum.sort_by(& &1.sort_order, :asc)
        |> Enum.map(fn f ->
          collection_id =
            if f.type == "relation" do
              resolve_collection_id_from_options(f.options)
            else
              nil
            end

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
            sort_order: f.sort_order,
            collectionId: collection_id
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

  # Resolve a relation field's target collection UUID from options.
  # Looks up the collection name stored in options["collection"] and returns its ID.
  defp resolve_collection_id_from_options(opts) do
    coll_name = opts["collection"]

    if is_binary(coll_name) and coll_name != "" do
      case CollectionRegistry.get(coll_name) do
        {:ok, coll} -> coll.id
        {:error, _} -> nil
      end
    else
      nil
    end
  end

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
