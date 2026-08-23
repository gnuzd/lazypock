defmodule LazypockWeb.CustomChannel do
  @moduledoc """
  Catch-all channel for custom realtime topics.

  Any topic that doesn't match the reserved `collection:*` / `collections`
  namespaces routes here (e.g. `chat:room1`, `notifications:user1`).
  Anonymous joins are allowed (PocketBase behavior); events are pushed by
  server-side hooks via `Lazypock.Realtime.Broadcaster.broadcast_custom/3`
  and are delivered verbatim to every subscriber of the topic.
  """

  use Phoenix.Channel

  # Reserved namespaces that must never route to this catch-all.
  @reserved_prefixes ["collection:", "collections"]

  @impl true
  def join(topic, _payload, socket) do
    cond do
      not is_binary(topic) or topic == "" ->
        {:error, %{reason: "Invalid topic"}}

      Enum.any?(@reserved_prefixes, &String.starts_with?(topic, &1)) ->
        {:error, %{reason: "Topic is reserved"}}

      String.length(topic) > 256 ->
        {:error, %{reason: "Topic too long"}}

      true ->
        {:ok, assign(socket, :topic, topic)}
    end
  end

  @impl true
  def handle_in("ping", _payload, socket) do
    {:reply, {:ok, %{ping: "pong"}}, socket}
  end
end
