defmodule Lazypock.Realtime.Broadcaster do
  @moduledoc """
  Broadcasts CRUD events to all subscribed Phoenix Channel clients.

  Called from the DynamicController after successful DB operations.
  """

  alias LazypockWeb.Endpoint

  @doc """
  Broadcasts a create event to all subscribers of a collection.
  """
  def broadcast_create(collection_name, record) do
    payload = %{
      action: "create",
      record: format_record(record, collection_name)
    }

    Endpoint.broadcast!("collection:#{collection_name}", "record_change", payload)
    Endpoint.broadcast!("collection:#{collection_name}:*", "record_change", payload)
  end

  @doc """
  Broadcasts an update event to all subscribers of a collection and the specific record.
  """
  def broadcast_update(collection_name, record) do
    payload = %{
      action: "update",
      record: format_record(record, collection_name)
    }

    Endpoint.broadcast!("collection:#{collection_name}", "record_change", payload)
    Endpoint.broadcast!("collection:#{collection_name}:*", "record_change", payload)
    Endpoint.broadcast!("collection:#{collection_name}:#{record["id"]}", "record_change", payload)
  end

  @doc """
  Broadcasts a delete event to all subscribers.
  """
  def broadcast_delete(collection_name, record_id) do
    payload = %{
      action: "delete",
      record: %{"id" => record_id}
    }

    Endpoint.broadcast!("collection:#{collection_name}", "record_change", payload)
    Endpoint.broadcast!("collection:#{collection_name}:*", "record_change", payload)
    Endpoint.broadcast!("collection:#{collection_name}:#{record_id}", "record_change", payload)
  end

  @doc """
  Broadcasts a collection CRUD event (create/update/delete) for admin purposes.
  """
  def broadcast_collection_event(action, collection_json) do
    payload = %{
      action: action,
      record: collection_json
    }

    Endpoint.broadcast!("admin:collections", "collection_change", payload)
  end

  defp format_record(record, collection_name) do
    record
    |> Map.put("collectionName", collection_name)
    |> Map.drop(["created_at", "updated_at"])
  end
end
