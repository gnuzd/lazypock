defmodule LazypockWeb.CollectionChannel do
  @moduledoc """
  Phoenix Channel for real-time collection subscriptions.

  Topics:
    `collection:name`       — all changes to a collection
    `collection:name:{id}`  — changes to a specific record
  """

  use Phoenix.Channel

  # Intercept server broadcasts so handle_out can filter the originating
  # connection (record_change is broadcast by Lazypock.Realtime.Broadcaster).
  intercept(["record_change"])

  @impl true
  def join("collection:" <> topic, payload, socket) do
    # topic can be: "posts", "posts:abc123", "posts:*"
    {collection_name, _record_id} = parse_topic(topic)

    case Lazypock.Collections.Registry.get(collection_name) do
      {:ok, _collection} ->
        # Check listRule for this user
        user = socket.assigns[:current_user]

        case Lazypock.Rules.Enforcer.authorize_list(collection_name, user) do
          {:ok, _} ->
            # Fire onRealtimeSubscribeRequest (PocketBase parity) —
            # handlers can reject via {:error, reason}
            case Lazypock.Hooks.Realtime.trigger_subscribe(
                   socket,
                   socket,
                   payload
                 ) do
              {:ok, _event, _after_funs} ->
                {:ok, assign(socket, :collection_name, collection_name)}

              {:error, reason} ->
                {:error, %{reason: format_reason(reason)}}
            end

          {:error, _reason} ->
            {:error, %{reason: "Access denied"}}
        end

      {:error, :not_found} ->
        {:error, %{reason: "Collection not found"}}
    end
  end

  @impl true
  def handle_in("ping", _payload, socket) do
    {:reply, {:ok, %{ping: "pong"}}, socket}
  end

  @doc """
  Intercepts server broadcasts before they reach the client.

  When a broadcast is tagged with `from_connection` (the connection id of
  the socket/HTTP request that performed the CRUD), the event is dropped
  for the originating connection and delivered to every other subscriber
  (including the same user's other tabs/devices). Clients without a
  connection id never match, so behavior is unchanged for third-party
  clients that don't send one. The internal `from_connection` field is
  stripped from what reaches clients.
  """
  @impl true
  def handle_out("record_change", %{"from_connection" => from} = payload, socket)
      when is_binary(from) do
    if from == socket.assigns[:connection_id] do
      {:noreply, socket}
    else
      push(socket, "record_change", Map.delete(payload, "from_connection"))
      {:noreply, socket}
    end
  end

  def handle_out("record_change", payload, socket) do
    push(socket, "record_change", payload)
    {:noreply, socket}
  end

  defp parse_topic(topic) do
    case String.split(topic, ":") do
      [collection] -> {collection, nil}
      [collection, id] -> {collection, id}
      [collection, id | _] -> {collection, id}
    end
  end

  defp format_reason(reason) when is_binary(reason), do: reason
  defp format_reason(reason), do: inspect(reason)
end
