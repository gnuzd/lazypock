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
          "superuser" => superuser_json(superuser)
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
          "superuser" => superuser_json(superuser)
        })

      {:error, reason} ->
        conn
        |> put_status(401)
        |> json(%{"code" => 401, "message" => reason, "data" => %{}})
    end
  end

  @doc """
  GET /api/me
  Returns the currently authenticated identity — works for BOTH superuser
  tokens and auth collection user tokens (PocketBase parity: superusers are
  records of the `_superusers` auth collection).

  Superuser tokens → `{id, email, collectionName: "_superusers", created, updated}`
  User tokens     → the auth record itself (`record`)
  Requires a valid JWT token in the Authorization header.
  """
  def me(conn, _params) do
    cond do
      conn.assigns[:current_superuser] ->
        json(conn, superuser_json(conn.assigns[:current_superuser]))

      conn.assigns[:current_user] ->
        json(conn, conn.assigns[:current_user])

      true ->
        conn
        |> put_status(401)
        |> json(%{"code" => 401, "message" => "Not authenticated", "data" => %{}})
    end
  end

  @doc """
  POST /api/_superusers/auth-with-password
  PocketBase-parity superuser login: `_superusers` behaves like an auth
  collection, so login goes through the standard `/:collection/auth-with-password`
  shape. This action is mounted as a static route (the dynamic auth route
  currently rejects system collections) and delegates to the standard flow.
  """
  def superuser_auth_with_password(conn, params) do
    email = params["identity"] || params["email"]
    password = params["password"]

    cond do
      is_nil(email) or email == "" ->
        conn
        |> put_status(400)
        |> json(%{
          "code" => 400,
          "message" => "Missing required field: identity or email",
          "data" => %{}
        })

      is_nil(password) or password == "" ->
        conn
        |> put_status(400)
        |> json(%{"code" => 400, "message" => "Missing required field: password", "data" => %{}})

      true ->
        do_superuser_auth_with_password(conn, email, password)
    end
  end

  defp do_superuser_auth_with_password(conn, email, password) do
    case Setup.authenticate(email, password) do
      {:ok, superuser} ->
        {:ok, token} = Token.generate_access_token(superuser)

        conn
        |> put_status(200)
        |> json(%{
          "token" => token,
          "record" => superuser_json(superuser)
        })

      {:error, _reason} ->
        conn
        |> put_status(401)
        |> json(%{"code" => 401, "message" => "Invalid email or password", "data" => %{}})
    end
  end

  @doc """
  GET /api/_superusers/auth-methods
  PocketBase parity — `_superusers` supports password auth (no OAuth2).
  """
  def superuser_auth_methods(conn, _params) do
    json(conn, %{
      "password" => true,
      "oauth2" => %{"providers" => []},
      "mfa" => %{}
    })
  end

  @doc """
  GET /api/superusers/check
  Returns whether any superuser exists (for the setup UI redirect).
  """
  def check(conn, _params) do
    json(conn, %{"has_superuser" => Setup.any_superuser?()})
  end

  defp superuser_json(superuser) do
    %{
      "id" => uuid_string(superuser.id),
      "email" => superuser.email,
      "collectionName" => "_superusers",
      "created" => superuser.created_at,
      "updated" => superuser.updated_at
    }
  end

  # Accepts both a canonical UUID string (from Repo.get) and a raw 16-byte
  # binary (from Setup.create_first's RETURNING) and normalises to a string.
  defp uuid_string(id) when is_binary(id) and byte_size(id) == 16 do
    Ecto.UUID.cast!(id)
  end

  defp uuid_string(id) when is_binary(id), do: id

  defp uuid_string(id), do: to_string(id)
end
