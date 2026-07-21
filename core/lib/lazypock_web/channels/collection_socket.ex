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

  channel "collection:*", LazypockWeb.CollectionChannel

  @impl true
  def connect(params, socket, _connect_info) do
    case params["token"] do
      nil ->
        # Allow unauthenticated connection for public collections
        {:ok, assign(socket, :current_user, nil)}

      token ->
        case Token.verify_token(token) do
          {:ok, claims} ->
            user = %{"id" => claims["id"], "email" => claims["email"], "role" => claims["type"]}
            {:ok, assign(socket, :current_user, user)}

          {:error, _reason} ->
            {:error, %{reason: "Invalid token"}}
        end
    end
  end

  @impl true
  def id(_socket), do: nil
end
