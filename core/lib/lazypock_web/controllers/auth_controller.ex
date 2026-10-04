defmodule LazypockWeb.AuthController do
  @moduledoc """
  Auth collection authentication endpoints (PocketBase-compatible).

  Handles login/refresh/methods for auth-type collections (e.g. `users`).
  Registration is handled by DynamicController.create (POST /api/users).
  """
  use LazypockWeb, :controller

  require Logger

  alias Lazypock.Collections.Registry
  alias Lazypock.Schemas.GenericRecord
  alias Lazypock.Auth.Token
  alias Lazypock.Auth.RateLimiter
  alias LazypockWeb.DynamicView

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
            # Format like every other record (adds `collectionId`/`collectionName`,
            # renames timestamps, strips password fields) so clients can derive the
            # auth collection from the response — PocketBase does the same.
            safe_user = DynamicView.format_item(user, collection_name)
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
          app_origin = oauth2_app_origin(conn)

          json(conn, %{
            "password" => true,
            "oauth2" => %{"providers" => oauth2_providers_payload(collection_name, app_origin)},
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

  defp oauth2_providers_payload(collection_name, app_origin) do
    Enum.map(Lazypock.Auth.OAuth2.providers(), fn {name, _cfg} ->
      case Lazypock.Auth.OAuth2.authorize_url(name) do
        {:ok, %{url: url, session_params: session_params}} ->
          state = session_params[:state] || session_params["state"]
          verifier = session_params[:code_verifier] || session_params["code_verifier"]

          # Store provider + collection + verifier server-side keyed by state so
          # the redirect callback can validate/consume the session, and record
          # the app origin the popup result must be posted back to.
          case Lazypock.Auth.OAuth2.store_session(
                 name,
                 collection_name,
                 verifier,
                 state,
                 app_origin
               ) do
            {:ok, _state} ->
              %{
                "name" => name,
                "authURL" => url,
                "state" => state,
                "codeVerifier" => verifier
              }

            {:error, :too_many_sessions} ->
              Logger.warning(
                "OAuth2 session store is at capacity; omitting provider #{name} from auth-methods"
              )

              nil
          end

        {:error, _} ->
          nil
      end
    end)
    |> Enum.reject(&is_nil/1)
  end

  # The front-end origin that initiated the flow. Browsers send `Origin` on
  # cross-origin fetches (and most non-GET same-origin ones); validate it against
  # the configured CORS allow-list. Fall back to this request's own origin so
  # same-origin deployments (e.g. the bundled Studio) keep working.
  #
  # The returned string is in the browser's own origin format (no default port)
  # so it can be used directly as a `postMessage` targetOrigin.
  defp oauth2_app_origin(conn) do
    raw = conn |> get_req_header("origin") |> List.first()
    raw = raw && String.trim(raw)
    normalized = raw && Lazypock.CORS.normalize_origin(raw)

    cond do
      is_binary(normalized) and Lazypock.CORS.origin_allowed?(URI.parse(normalized), :any) ->
        raw

      true ->
        request_origin(conn)
    end
  end

  defp request_origin(conn) do
    scheme = Atom.to_string(conn.scheme)
    default? = (scheme == "http" and conn.port == 80) or (scheme == "https" and conn.port == 443)

    if default? do
      "#{scheme}://#{conn.host}"
    else
      "#{scheme}://#{conn.host}:#{conn.port}"
    end
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
        # Direct code exchange (PB authWithOAuth2Code): the client provides the
        # codeVerifier but not the state, so PKCE is the binding (skip state).
        session_params = %{
          state: false,
          code_verifier: code_verifier,
          redirect_url: redirect_url
        }

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
        safe_user = DynamicView.format_item(record, collection_name)
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
        handle_successful_login(conn, collection_name, user)

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

  The provider redirects the browser here with `?code=...&state=...`. The
  pending session created by `auth-methods` is validated and consumed, then this
  endpoint serves a tiny HTML page that relays **only the authorization code**
  back to the popup opener via `postMessage`, targeted at the origin captured
  when the flow started.

  The code is exchanged — and the auth record created/linked — by the SDK calling
  `POST /api/:collection/auth-with-oauth2` (`authWithOAuth2Code`). Keeping the
  token and record out of this page means an injection here cannot leak a
  session, and it lets the client forward `createData` on first sign-up.
  """
  def oauth2_redirect(conn, params) do
    {app_origin, session?} =
      case Lazypock.Auth.OAuth2.take_session(params["state"] || "") do
        {:ok, _provider, _collection, _code_verifier, app_origin} -> {app_origin, true}
        {:error, _reason} -> {nil, false}
      end

    provider_error = params["error"]
    code = params["code"]

    cond do
      not session? ->
        send_redirect_error(conn, nil, "Invalid or expired OAuth2 session")

      provider_error not in [nil, ""] ->
        Logger.warning("OAuth2 provider returned an error: #{provider_error}")
        send_redirect_error(conn, app_origin, "The OAuth2 provider rejected the sign-in request.")

      code in [nil, ""] ->
        send_redirect_error(conn, app_origin, "Missing authorization code")

      true ->
        send_redirect_code(conn, app_origin, code, params["state"])
    end
  end

  defp send_redirect_code(conn, app_origin, code, state) do
    payload = %{"type" => "lazypock:oauth2", "code" => code, "state" => state}

    send_oauth2_result(conn, 200, payload, app_origin)
  end

  defp send_redirect_error(conn, app_origin, message) do
    payload = %{"type" => "lazypock:oauth2:error", "message" => message}

    send_oauth2_result(conn, 400, payload, app_origin)
  end

  # Render the popup bridge page. The payload is JSON-encoded with HTML-safe
  # escaping and covered by a per-response CSP nonce, so provider/record data
  # can never break out of the inline script. Only the single-use `code` is
  # relayed — never a token or user record.
  defp send_oauth2_result(conn, status, payload, app_origin) do
    target_origin = app_origin || request_origin(conn)
    nonce = Base.url_encode64(:crypto.strong_rand_bytes(18), padding: false)
    payload_json = Jason.encode!(payload, escape: :html_safe)
    origin_json = Jason.encode!(target_origin)

    html = """
    <!doctype html>
    <html>
      <head><meta charset="utf-8"><title>LazyPock sign-in</title></head>
      <body>
        <script nonce="#{nonce}">
          (function () {
            var payload = #{payload_json};
            if (window.opener) {
              window.opener.postMessage(payload, #{origin_json});
              window.close();
            } else {
              document.body.textContent = "Sign-in complete. You can close this window and return to the app.";
            }
          })();
        </script>
      </body>
    </html>
    """

    conn
    |> put_resp_content_type("text/html")
    |> put_resp_header(
      "content-security-policy",
      "default-src 'none'; script-src 'nonce-#{nonce}'; base-uri 'none'; form-action 'none'"
    )
    |> put_resp_header("cache-control", "no-store")
    |> put_resp_header("x-content-type-options", "nosniff")
    |> put_resp_header("referrer-policy", "no-referrer")
    |> send_resp(status, html)
  end

  defp handle_successful_login(conn, collection_name, user) do
    {:ok, token} = Token.generate_user_token(user, collection_name)

    # Same record shape as every other endpoint: password fields are stripped
    # from the collection schema and `collectionId`/`collectionName` are added.
    safe_user = DynamicView.format_item(user, collection_name)

    conn
    |> put_status(200)
    |> json(%{
      "token" => token,
      "record" => safe_user
    })
  end
end
