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

  @impl true
  def connect(params, socket, _connect_info) do
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
end
