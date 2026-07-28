defmodule Lazypock.Auth.Plug do
  @moduledoc """
  Plug for authenticating requests via JWT bearer tokens.

  Supports two token types:
  - **Superuser tokens** — assigns `current_superuser` (Ecto struct)
  - **Auth collection user tokens** — assigns `current_user` (record map)

  The Enforcer uses `current_superuser` for superuser bypass and
  `current_user` for `@request.auth.*` rule resolution.
  """

  import Plug.Conn

  alias Lazypock.Auth.Token
  alias Lazypock.Schemas.GenericRecord

  @doc """
  Authenticates the request. Called from the router pipeline.
  """
  def init(opts), do: opts

  def call(conn, _opts) do
    conn
    |> get_token()
    |> case do
      {:ok, token} ->
        # Try superuser token first
        case Token.verify_token(token) do
          {:ok, claims} ->
            superuser = Lazypock.Repo.get(Lazypock.Auth.SuperUser, claims["id"])

            if superuser do
              conn
              |> assign(:current_superuser, superuser)
              |> assign(:current_superuser_claims, claims)
              |> assign(:current_user, nil)
            else
              conn |> assign_nil()
            end

          {:error, _reason} ->
            # Try auth collection user token
            case Token.verify_user_token(token) do
              {:ok, claims} ->
                collection_name = claims["collectionName"]
                user_id = claims["id"]

                if collection_name && user_id do
                  user = GenericRecord.get(collection_name, user_id)

                  if user do
                    conn
                    |> assign(:current_user, user)
                    |> assign(:current_user_claims, claims)
                    |> assign(:current_superuser, nil)
                  else
                    conn |> assign_nil()
                  end
                else
                  conn |> assign_nil()
                end

              {:error, _reason} ->
                conn |> assign_nil()
            end
        end

      :none ->
        conn |> assign_nil()
    end
  end

  @doc """
  Returns true if the request is authenticated as a superuser.
  """
  def authenticated?(conn) do
    !!conn.assigns[:current_superuser]
  end

  @doc """
  Returns the currently authenticated user record (auth collection) or nil.
  """
  def current_user(conn) do
    conn.assigns[:current_user]
  end

  @doc """
  Returns the currently authenticated superuser or nil.
  """
  def current_superuser(conn) do
    conn.assigns[:current_superuser]
  end

  defp assign_nil(conn) do
    conn
    |> assign(:current_superuser, nil)
    |> assign(:current_superuser_claims, nil)
    |> assign(:current_user, nil)
    |> assign(:current_user_claims, nil)
  end

  defp get_token(conn) do
    case get_req_header(conn, "authorization") do
      ["Bearer " <> token | _] -> {:ok, token}
      ["bearer " <> token | _] -> {:ok, token}
      _ -> :none
    end
  end
end
