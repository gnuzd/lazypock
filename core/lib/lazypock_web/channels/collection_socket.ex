defmodule LazypockWeb.CollectionSocket do
  @moduledoc """
  Phoenix Socket for real-time collection subscriptions.

  Clients connect with a JWT token and subscribe to collection topics:
    - `collection:posts`       — all changes to "posts" collection
    - `collection:posts:*`     — all changes to "posts" collection
    - `collection:posts:abc123` — changes to a specific record

  The socket authenticates via the JWT token passed in `params.token`.
  """

  use Phoenix.Socket

  alias Lazypock.Auth.Token

  channel("collection:*", LazypockWeb.CollectionChannel)
  channel("collections", LazypockWeb.AdminChannel)
  # Custom channels — declared LAST: any topic that doesn't match the
  # collection patterns above (e.g. "chat:room1") routes here. Anonymous
  # joins allowed (PocketBase behavior); broadcasts come from the server
  # side via Lazypock.Realtime.Broadcaster.broadcast_custom/4.
  channel("*", LazypockWeb.CustomChannel)

  @impl true
  def connect(params, socket, _connect_info) do
    # Optional per-connection id (client-generated, e.g. a browser tab
    # UUID) used to exclude the originating connection from its own
    # realtime broadcasts. Sent by the SDK as the `connectionId` socket
    # query param and as the `X-Connection-Id` header on HTTP requests.
    socket =
      assign(
        socket,
        :connection_id,
        normalize_connection_id(params["connectionId"] || params["connection_id"])
      )

    case params["token"] do
      nil ->
        # Allow unauthenticated connection for public collections
        {:ok, assign(socket, :current_user, nil)}

      token ->
        case Token.verify_token(token) do
          {:ok, claims} ->
            # Superuser token — assign an actual SuperUser struct so the rule
            # enforcer's superuser bypass (struct-based) applies to realtime
            # subscriptions, matching PocketBase where superusers can
            # subscribe to any collection regardless of rules.
            superuser = %Lazypock.Auth.SuperUser{
              id: claims["id"],
              email: claims["email"] || ""
            }

            {:ok, assign(socket, :current_user, superuser)}

          {:error, _reason} ->
            # Try auth collection user token
            case Token.verify_user_token(token) do
              {:ok, claims} ->
                user = %{
                  "id" => claims["id"],
                  "email" => claims["email"] || "",
                  "role" => "user",
                  "collectionName" => claims["collectionName"]
                }

                {:ok, assign(socket, :current_user, user)}

              {:error, _reason} ->
                {:error, %{reason: "Invalid token"}}
            end
        end
    end
  end

  @impl true
  def id(_socket), do: nil

  # Opaque, length-capped connection id. Nil when the client didn't send one.
  defp normalize_connection_id(nil), do: nil
  defp normalize_connection_id(id) when is_binary(id), do: String.slice(id, 0, 128)
  defp normalize_connection_id(_), do: nil
end
