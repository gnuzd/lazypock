defmodule LazypockWeb.SuperUserController do
  use LazypockWeb, :controller

  alias Lazypock.Auth.Setup
  alias Lazypock.Auth.Token

  @doc """
  POST /api/superusers/setup
  Creates the first superuser. Only works if no superuser exists yet.
  """
  def setup(conn, %{"email" => email, "password" => password}) do
    case Setup.create_first(email, password) do
      {:ok, superuser} ->
        {:ok, jwt} = Token.generate_access_token(superuser)

        conn
        |> put_status(201)
        |> json(%{
          "token" => jwt,
          "superuser" => %{
            "id" => superuser.id,
            "email" => superuser.email
          }
        })

      {:error, reason} ->
        conn
        |> put_status(400)
        |> json(%{"code" => 400, "message" => reason, "data" => %{}})
    end
  end

  @doc """
  POST /api/superusers/login
  Authenticates and returns a JWT token.
  """
  def login(conn, %{"email" => email, "password" => password}) do
    case Setup.authenticate(email, password) do
      {:ok, superuser} ->
        {:ok, jwt} = Token.generate_access_token(superuser)

        conn
        |> put_status(200)
        |> json(%{
          "token" => jwt,
          "superuser" => %{
            "id" => superuser.id,
            "email" => superuser.email
          }
        })

      {:error, reason} ->
        conn
        |> put_status(401)
        |> json(%{"code" => 401, "message" => reason, "data" => %{}})
    end
  end

  @doc """
  GET /api/superusers/me
  Returns the currently authenticated superuser.
  Requires a valid JWT token in the Authorization header.
  """
  def me(conn, _params) do
    case conn.assigns[:current_superuser] do
      nil ->
        conn
        |> put_status(401)
        |> json(%{"code" => 401, "message" => "Not authenticated", "data" => %{}})

      superuser ->
        json(conn, %{
          "id" => superuser.id,
          "email" => superuser.email
        })
    end
  end

  @doc """
  GET /api/superusers/check
  Returns whether any superuser exists (for the setup UI redirect).
  """
  def check(conn, _params) do
    json(conn, %{"has_superuser" => Setup.any_superuser?()})
  end
end
