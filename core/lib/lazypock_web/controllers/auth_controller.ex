defmodule LazypockWeb.AuthController do
  @moduledoc """
  Auth collection authentication endpoints (PocketBase-compatible).

  Handles login/refresh/methods for auth-type collections (e.g. `users`).
  Registration is handled by DynamicController.create (POST /api/users).
  """
  use LazypockWeb, :controller

  alias Lazypock.Collections.Registry
  alias Lazypock.Schemas.GenericRecord
  alias Lazypock.Auth.Token
  alias Lazypock.Auth.RateLimiter

  @doc """
  POST /api/:collection/auth-with-password

  PocketBase-compatible email+password login for auth collections.
  Returns a JWT token and user record on success.
  """
  def auth_with_password(conn, %{"collection" => collection_name} = params) do
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
        ip = RateLimiter.ip_from_conn(conn)

        case RateLimiter.check_rate(ip, collection_name, email) do
          :ok ->
            do_auth_with_password(conn, collection_name, email, password)

          {:error, :rate_limited} ->
            conn
            |> put_status(429)
            |> json(%{
              "code" => 429,
              "message" => "Too many login attempts. Please try again later.",
              "data" => %{}
            })
        end
    end
  end

  @doc """
  POST /api/:collection/auth-refresh

  Refreshes the auth token for an already authenticated user.
  Requires a valid JWT in the Authorization header.
  Returns a fresh token and updated user record.
  """
  def auth_refresh(conn, %{"collection" => collection_name}) do
    # Verify the URL collection matches the token's claim (prevents cross-collection contamination)
    claims = conn.assigns[:current_user_claims]

    cond do
      is_nil(conn.assigns[:current_user]) ->
        conn
        |> put_status(401)
        |> json(%{"code" => 401, "message" => "Not authenticated", "data" => %{}})

      is_nil(claims) or claims["collectionName"] != collection_name ->
        conn
        |> put_status(403)
        |> json(%{
          "code" => 403,
          "message" => "Token does not match this collection",
          "data" => %{}
        })

      true ->
        do_auth_refresh(conn, collection_name)
    end
  end

  defp do_auth_refresh(conn, collection_name) do
    case Registry.get(collection_name) do
      {:ok, %{type: "auth"} = collection} ->
        user = conn.assigns[:current_user]

        # Fire onRecordAuthRefreshRequest (PocketBase parity)
        case Lazypock.Hooks.Request.trigger_record_auth_refresh(
               conn,
               collection_name,
               collection,
               user
             ) do
          {:ok, _event} ->
            password_field = find_password_field(collection)
            safe_user = Map.drop(user, [password_field])
            {:ok, token} = Token.generate_user_token(user, collection_name)

            conn
            |> put_status(200)
            |> json(%{
              "token" => token,
              "record" => safe_user
            })

          {:error, reason} ->
            conn
            |> put_status(400)
            |> json(%{"code" => 400, "message" => to_string(reason), "data" => %{}})
        end

      {:ok, _} ->
        conn
        |> put_status(400)
        |> json(%{"code" => 400, "message" => "Not an auth collection", "data" => %{}})

      {:error, :not_found} ->
        conn
        |> put_status(404)
        |> json(%{"code" => 404, "message" => "Collection not found", "data" => %{}})
    end
  end

  @doc """
  GET /api/:collection/auth-methods

  Returns the available auth methods for a collection (PocketBase-compatible).
  """
  def auth_methods(conn, %{"collection" => collection_name}) do
    case Registry.get(collection_name) do
      {:ok, collection} ->
        if collection.type == "auth" do
          json(conn, %{
            "password" => true,
            "oauth2" => %{},
            "mfa" => %{}
          })
        else
          conn
          |> put_status(400)
          |> json(%{"code" => 400, "message" => "Not an auth collection", "data" => %{}})
        end

      {:error, :not_found} ->
        conn
        |> put_status(404)
        |> json(%{"code" => 404, "message" => "Collection not found", "data" => %{}})
    end
  end

  defp do_auth_with_password(conn, collection_name, email, password) do
    ip = RateLimiter.ip_from_conn(conn)

    # First check the collection exists and is auth type
    case Registry.get(collection_name) do
      {:ok, %{type: "auth"} = collection} ->
        # Fire onRecordAuthWithPasswordRequest (PocketBase parity)
        case Lazypock.Hooks.Request.trigger_record_auth_with_password(
               conn,
               collection_name,
               collection,
               nil,
               email,
               find_email_field(collection),
               password
             ) do
          {:ok, _event} ->
            find_user_by_email(conn, collection_name, email, collection, password, ip)

          {:error, reason} ->
            conn
            |> put_status(400)
            |> json(%{"code" => 400, "message" => to_string(reason), "data" => %{}})
        end

      {:ok, _} ->
        conn
        |> put_status(400)
        |> json(%{"code" => 400, "message" => "Not an auth collection", "data" => %{}})

      {:error, :not_found} ->
        conn
        |> put_status(404)
        |> json(%{"code" => 404, "message" => "Collection not found", "data" => %{}})
    end
  end

  defp find_user_by_email(conn, collection_name, email, collection, password, ip) do
    # Find the email field dynamically from collection schema
    email_field = find_email_field(collection)

    records =
      GenericRecord.all_where(
        collection_name,
        ~s("#{email_field}" = $1),
        [email]
      )

    case records do
      [user | _] ->
        # First matching user
        verify_password(conn, collection_name, user, password, collection, ip, email)

      [] ->
        RateLimiter.record_attempt(ip, collection_name, email, :failure)

        conn
        |> put_status(401)
        |> json(%{"code" => 401, "message" => "Invalid email or password", "data" => %{}})
    end
  end

  defp find_email_field(collection) do
    email_fields =
      (collection.fields || [])
      |> Enum.filter(fn f -> f.type == "email" end)
      |> Enum.map(fn f -> f.name end)

    case email_fields do
      [name | _] -> name
      [] -> "email"
    end
  end

  defp find_password_field(collection) do
    password_fields =
      (collection.fields || [])
      |> Enum.filter(fn f -> f.type == "password" end)
      |> Enum.map(fn f -> f.name end)

    case password_fields do
      [name | _] -> name
      [] -> "password_hash"
    end
  end

  defp verify_password(conn, collection_name, user, password, collection, ip, email) do
    # Find the password field name from the collection schema
    password_field = find_password_field(collection)
    password_hash = user[password_field]

    cond do
      is_nil(password_hash) ->
        RateLimiter.record_attempt(ip, collection_name, email, :failure)

        conn
        |> put_status(401)
        |> json(%{"code" => 401, "message" => "Invalid email or password", "data" => %{}})

      Bcrypt.verify_pass(password, password_hash) ->
        RateLimiter.record_attempt(ip, collection_name, email, :success)
        handle_successful_login(conn, collection_name, user, password_field)

      true ->
        RateLimiter.record_attempt(ip, collection_name, email, :failure)

        conn
        |> put_status(401)
        |> json(%{"code" => 401, "message" => "Invalid email or password", "data" => %{}})
    end
  end

  defp handle_successful_login(conn, collection_name, user, password_field) do
    {:ok, token} = Token.generate_user_token(user, collection_name)

    # Strip password field(s) from response
    safe_user = Map.drop(user, [password_field])

    conn
    |> put_status(200)
    |> json(%{
      "token" => token,
      "record" => safe_user
    })
  end
end
