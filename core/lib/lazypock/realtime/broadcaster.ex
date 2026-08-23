defmodule Lazypock.Realtime.Broadcaster do
  @moduledoc """
  Broadcasts CRUD events to all subscribed Phoenix Channel clients.

  Called from the DynamicController after successful DB operations.
  """

  alias LazypockWeb.Endpoint

  @doc """
  Broadcasts a create event to all subscribers of a collection.

  `from_connection` is the connection id of the socket/HTTP request that
  performed the CRUD — the originating connection will not receive its own
  event (see `LazypockWeb.CollectionChannel.handle_out/3`). `nil` keeps
  the current behavior (everyone receives it).
  """
  def broadcast_create(collection_name, record, from_connection \\ nil) do
    payload =
      %{
        action: "create",
        record: format_record(record, collection_name)
      }
      |> put_from_connection(from_connection)

    Endpoint.broadcast!("collection:#{collection_name}", "record_change", payload)
    Endpoint.broadcast!("collection:#{collection_name}:*", "record_change", payload)
  end

  @doc """
  Broadcasts an update event to all subscribers of a collection and the specific record.
  """
  def broadcast_update(collection_name, record, from_connection \\ nil) do
    payload =
      %{
        action: "update",
        record: format_record(record, collection_name)
      }
      |> put_from_connection(from_connection)

    Endpoint.broadcast!("collection:#{collection_name}", "record_change", payload)
    Endpoint.broadcast!("collection:#{collection_name}:*", "record_change", payload)
    Endpoint.broadcast!("collection:#{collection_name}:#{record["id"]}", "record_change", payload)
  end

  @doc """
  Broadcasts a delete event to all subscribers.
  """
  def broadcast_delete(collection_name, record_id, from_connection \\ nil) do
    payload =
      %{
        action: "delete",
        record: %{"id" => record_id}
      }
      |> put_from_connection(from_connection)

    Endpoint.broadcast!("collection:#{collection_name}", "record_change", payload)
    Endpoint.broadcast!("collection:#{collection_name}:*", "record_change", payload)
    Endpoint.broadcast!("collection:#{collection_name}:#{record_id}", "record_change", payload)
  end

  @doc """
  Broadcasts a collection CRUD event (create/update/delete) for admin purposes.
  Topic: "collections", event name is the action itself.
  """
  def broadcast_collection_event(action, collection_json) do
    Endpoint.broadcast!("collections", action, collection_json)
  end

  @doc """
  Broadcasts an arbitrary event on a custom channel topic (e.g. "chat:room1").

  For use from hooks / custom API routes. The topic must not be in the
  reserved `collection:*` / `collections` namespaces — those are handled by
  `broadcast_*/3` and `broadcast_collection_event/2`. Any connected client
  can join a custom topic (anonymous allowed, PocketBase behavior) and the
  event is delivered verbatim to every subscriber.
  """
  def broadcast_custom(topic, event, payload)
      when is_binary(topic) and is_binary(event) and is_map(payload) do
    Endpoint.broadcast!(topic, event, payload)
  end

  defp put_from_connection(payload, from_connection) when is_binary(from_connection) do
    Map.put(payload, "from_connection", from_connection)
  end

  defp put_from_connection(payload, _from_connection), do: payload

  defp format_record(record, collection_name) do
    record
    |> Map.put("collectionName", collection_name)
    |> Map.put("created", Map.get(record, "created_at"))
    |> Map.put("updated", Map.get(record, "updated_at"))
    |> Map.drop(["created_at", "updated_at"])
  end
end
