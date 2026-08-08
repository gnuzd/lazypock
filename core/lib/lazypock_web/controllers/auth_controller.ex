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

  When OAuth2 providers are configured, `oauth2.providers` contains each
  provider's `name`, `authURL`, `state`, and `codeVerifier` (PKCE).
  """
  def auth_methods(conn, %{"collection" => collection_name}) do
    case Registry.get(collection_name) do
      {:ok, collection} ->
        if collection.type == "auth" do
          json(conn, %{
            "password" => true,
            "oauth2" => %{"providers" => oauth2_providers_payload(collection_name)},
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

  @doc """
  POST /api/:collection/auth-with-oauth2

  PocketBase-compatible OAuth2 code exchange. Body:

      {"provider": "google", "code": "...", "codeVerifier": "...",
       "redirectUrl": "http://localhost:4000/api/oauth2-redirect",
       "createData": {}}

  Exchanges the authorization code for tokens + user info, links (or finds)
  the external auth, upserts the auth record, and returns a JWT.
  """
  def auth_with_oauth2(conn, %{"collection" => collection_name} = params) do
    provider = params["provider"]
    code = params["code"]
    code_verifier = params["codeVerifier"]
    redirect_url = params["redirectUrl"]
    create_data = params["createData"] || %{}

    cond do
      is_nil(provider) or provider == "" ->
        conn
        |> put_status(400)
        |> json(%{"code" => 400, "message" => "Missing required field: provider", "data" => %{}})

      is_nil(code) or code == "" ->
        conn
        |> put_status(400)
        |> json(%{"code" => 400, "message" => "Missing required field: code", "data" => %{}})

      is_nil(code_verifier) or code_verifier == "" ->
        conn
        |> put_status(400)
        |> json(%{
          "code" => 400,
          "message" => "Missing required field: codeVerifier",
          "data" => %{}
        })

      is_nil(redirect_url) or redirect_url == "" ->
        conn
        |> put_status(400)
        |> json(%{
          "code" => 400,
          "message" => "Missing required field: redirectUrl",
          "data" => %{}
        })

      true ->
        do_auth_with_oauth2(
          conn,
          collection_name,
          provider,
          code,
          code_verifier,
          redirect_url,
          create_data
        )
    end
  end

  defp oauth2_providers_payload(collection_name) do
    Enum.map(Lazypock.Auth.OAuth2.providers(), fn {name, _cfg} ->
      case Lazypock.Auth.OAuth2.authorize_url(name) do
        {:ok, %{url: url, session_params: session_params}} ->
          state = session_params[:state] || session_params["state"]
          verifier = session_params[:code_verifier] || session_params["code_verifier"]

          # Store provider + collection + verifier server-side keyed by state
          # so the redirect callback can recover them (PocketBase parity).
          Lazypock.Auth.OAuth2.store_session(name, collection_name, verifier, state)

          %{
            "name" => name,
            "authURL" => url,
            "state" => state,
            "codeVerifier" => verifier
          }

        {:error, _} ->
          nil
      end
    end)
    |> Enum.reject(&is_nil/1)
  end

  defp do_auth_with_oauth2(
         conn,
         collection_name,
         provider,
         code,
         code_verifier,
         redirect_url,
         create_data
       ) do
    case Registry.get(collection_name) do
      {:ok, %{type: "auth"} = collection} ->
        # Direct code exchange (PB authWithOAuth2Code): client provides the
        # codeVerifier but not the state, so skip state verification.
        session_params = %{state: false, code_verifier: code_verifier}

        case Lazypock.Auth.OAuth2.callback(provider, %{"code" => code}, session_params) do
          {:ok, %{user: oauth2_user, token: oauth2_token}} ->
            provider_id = oauth2_user["sub"] || oauth2_user["id"]

            # Fire onRecordAuthWithOAuth2Request (PocketBase parity)
            case Lazypock.Hooks.Request.trigger_record_auth_with_oauth2(
                   conn,
                   collection_name,
                   collection,
                   provider,
                   oauth2_token,
                   nil,
                   oauth2_user,
                   create_data,
                   false
                 ) do
              {:ok, _event} ->
                finish_oauth2_login(
                  conn,
                  collection_name,
                  collection,
                  provider,
                  provider_id,
                  oauth2_user,
                  oauth2_token,
                  create_data,
                  redirect_url
                )

              {:error, reason} ->
                conn
                |> put_status(400)
                |> json(%{"code" => 400, "message" => to_string(reason), "data" => %{}})
            end

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

  defp finish_oauth2_login(
         conn,
         collection_name,
         collection,
         provider,
         provider_id,
         oauth2_user,
         oauth2_token,
         create_data,
         _redirect_url
       ) do
    case Lazypock.Auth.OAuth2.find_or_create_record(
           collection_name,
           collection,
           provider,
           provider_id,
           oauth2_user,
           create_data
         ) do
      {:ok, %{record: record, is_new: is_new}} ->
        password_field = find_password_field(collection)
        safe_user = Map.drop(record, [password_field])
        {:ok, token} = Token.generate_user_token(record, collection_name)

        meta = %{
          "id" => record["id"],
          "name" => oauth2_user["name"],
          "email" => oauth2_user["email"],
          "isNew" => is_new,
          "avatarURL" => oauth2_user["picture"],
          "rawUser" => oauth2_user,
          "accessToken" => oauth2_token["access_token"],
          "refreshToken" => oauth2_token["refresh_token"],
          "expiry" => oauth2_token["expires_at"]
        }

        conn
        |> put_status(200)
        |> json(%{"token" => token, "record" => safe_user, "meta" => meta})

      {:error, reason} ->
        conn
        |> put_status(400)
        |> json(%{"code" => 400, "message" => to_string(reason), "data" => %{}})
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

  @doc """
  GET /api/oauth2-redirect

  OAuth2 provider redirect callback (PocketBase parity).

  The provider redirects the browser here with `?code=...&state=...`.
  This endpoint exchanges the code, links/upserts the auth record, and
  serves a tiny HTML page that posts the result back to the popup opener
  via `postMessage` (matching the JS SDK's `authWithOAuth2` popup flow).
  """
  def oauth2_redirect(conn, params) do
    code = params["code"]
    state = params["state"]

    if is_nil(code) or code == "" do
      send_redirect_error(conn, "Missing authorization code")
    else
      # Recover provider + collection + code_verifier from the session store
      case Lazypock.Auth.OAuth2.take_session(state || "") do
        {:ok, provider, collection_name, code_verifier} ->
          handle_oauth2_redirect(conn, provider, collection_name, code, code_verifier)

        {:error, reason} ->
          send_redirect_error(conn, "Invalid or expired OAuth2 session (#{reason})")
      end
    end
  end

  defp handle_oauth2_redirect(conn, provider, collection_name, code, code_verifier) do
    case Registry.get(collection_name) do
      {:ok, %{type: "auth"} = collection} ->
        session_params = %{state: false, code_verifier: code_verifier}

        case Lazypock.Auth.OAuth2.callback(provider, %{"code" => code}, session_params) do
          {:ok, %{user: oauth2_user, token: _oauth2_token}} ->
            provider_id = oauth2_user["sub"] || oauth2_user["id"]

            case Lazypock.Auth.OAuth2.find_or_create_record(
                   collection_name,
                   collection,
                   provider,
                   provider_id,
                   oauth2_user,
                   %{}
                 ) do
              {:ok, %{record: record, is_new: is_new}} ->
                password_field = find_password_field(collection)
                safe_user = Map.drop(record, [password_field])
                {:ok, token} = Token.generate_user_token(record, collection_name)

                result = %{
                  "token" => token,
                  "record" => safe_user,
                  "meta" => %{
                    "id" => record["id"],
                    "name" => oauth2_user["name"],
                    "email" => oauth2_user["email"],
                    "isNew" => is_new,
                    "avatarURL" => oauth2_user["picture"],
                    "rawUser" => oauth2_user
                  }
                }

                send_redirect_result(conn, result)

              {:error, reason} ->
                send_redirect_error(conn, to_string(reason))
            end

          {:error, reason} ->
            send_redirect_error(conn, to_string(reason))
        end

      _ ->
        send_redirect_error(conn, "Collection not found or not an auth collection")
    end
  end

  defp send_redirect_result(conn, result) do
    json = Jason.encode!(result)

    html = """
    <!doctype html>
    <html>
      <body>
        <script>
          const result = #{json};
          if (window.opener) {
            window.opener.postMessage({type: 'lazypock:oauth2', result}, window.location.origin);
            window.close();
          } else {
            document.body.textContent = JSON.stringify(result);
          }
        </script>
      </body>
    </html>
    """

    conn
    |> put_resp_content_type("text/html")
    |> send_resp(200, html)
  end

  defp send_redirect_error(conn, message) do
    json = Jason.encode!(%{"code" => 400, "message" => message, "data" => %{}})

    html = """
    <!doctype html>
    <html>
      <body>
        <script>
          const result = #{json};
          if (window.opener) {
            window.opener.postMessage({type: 'lazypock:oauth2:error', result}, window.location.origin);
            window.close();
          } else {
            document.body.textContent = JSON.stringify(result);
          }
        </script>
      </body>
    </html>
    """

    conn
    |> put_resp_content_type("text/html")
    |> send_resp(400, html)
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
