defmodule Lazypock.Auth.Plug do
  @moduledoc """
  Plug for authenticating requests via JWT bearer tokens.

  Extracts and validates the token from the Authorization header,
  then assigns `current_superuser` to the conn if valid.
  """

  import Plug.Conn

  @doc """
  Authenticates the request. Called from the router pipeline.
  """
  def init(opts), do: opts

  def call(conn, _opts) do
    conn
    |> get_token()
    |> case do
      {:ok, token} ->
        case Lazypock.Auth.Token.verify_token(token) do
          {:ok, claims} ->
            superuser = Lazypock.Repo.get(Lazypock.Auth.SuperUser, claims["id"])

            if superuser do
              conn
              |> assign(:current_superuser, superuser)
              |> assign(:current_superuser_claims, claims)
            else
              conn |> assign(:current_superuser, nil)
            end

          {:error, _reason} ->
            conn |> assign(:current_superuser, nil)
        end

      :none ->
        conn |> assign(:current_superuser, nil)
    end
  end

  @doc """
  Returns true if the request is authenticated as a superuser.
  """
  def authenticated?(conn) do
    !!conn.assigns[:current_superuser]
  end

  defp get_token(conn) do
    case get_req_header(conn, "authorization") do
      ["Bearer " <> token | _] -> {:ok, token}
      ["bearer " <> token | _] -> {:ok, token}
      _ -> :none
    end
  end
end
