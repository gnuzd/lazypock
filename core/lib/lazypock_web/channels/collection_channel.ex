defmodule LazypockWeb.CollectionChannel do
  @moduledoc """
  Phoenix Channel for real-time collection subscriptions.

  Topics:
    `collection:name`       — all changes to a collection
    `collection:name:{id}`  — changes to a specific record
  """

  use Phoenix.Channel

  @impl true
  def join("collection:" <> topic, _payload, socket) do
    # topic can be: "posts", "posts:abc123", "posts:*"
    {collection_name, _record_id} = parse_topic(topic)

    case Lazypock.Collections.Registry.get(collection_name) do
      {:ok, _collection} ->
        # Check listRule for this user
        user = socket.assigns[:current_user]

        case Lazypock.Rules.Enforcer.authorize_list(collection_name, user) do
          {:ok, _} ->
            {:ok, assign(socket, :collection_name, collection_name)}

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

  defp parse_topic(topic) do
    case String.split(topic, ":") do
      [collection] -> {collection, nil}
      [collection, id] -> {collection, id}
      [collection, id | _] -> {collection, id}
    end
  end
end
