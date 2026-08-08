defmodule LazypockWeb.OAuth2FlowTest do
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Schema.DDL
  alias Lazypock.Schemas.GenericRecord
  alias Lazypock.Collections.Registry
  alias Lazypock.Auth.OAuth2

  # Mock strategy that returns fixed user data
  # Mock strategy that returns fixed user data without any network access.
  # Implements `Assent.Strategy` directly (no `use OAuth2.Base`) so the
  # `callback/2` override actually takes effect (the base macro's generated
  # clause is not overridable).
  defmodule MockStrategy do
    @moduledoc false
    @behaviour Assent.Strategy

    alias Assent.Strategy, as: Helpers

    @impl Assent.Strategy
    def authorize_url(config) do
      base_url = Keyword.get(config, :base_url, "https://example.test")

      state =
        config
        |> Keyword.get(:session_params, %{})
        |> Map.get(:state) ||
          :crypto.strong_rand_bytes(16) |> Base.url_encode64(padding: false)

      code_verifier = :crypto.strong_rand_bytes(32) |> Base.url_encode64(padding: false)

      {:ok,
       %{
         url: "#{base_url}/oauth/authorize?state=#{state}&code_challenge=#{code_verifier}",
         session_params: %{state: state, code_verifier: code_verifier}
       }}
    end

    @impl Assent.Strategy
    def callback(_config, _params) do
      {:ok,
       %{
         user: %{
           "id" => "ext-user-123",
           "name" => "Mock User",
           "email" => "mock@example.com",
           "avatar_url" => "https://example.test/avatar.png"
         },
         token: %{"access_token" => "mock-token", "token_type" => "Bearer"}
       }}
    end

    @impl true
    def fetch_user(_config, _token) do
      {:ok,
       %{
         "id" => "ext-user-123",
         "name" => "Mock User",
         "email" => "mock@example.com",
         "avatar_url" => "https://example.test/avatar.png"
       }}
    end
  end

  setup do
    OAuth2.ensure_external_auths_table!()
    OAuth2.ensure_session_table!()

    # Create an auth collection ("oauth_users") with email + password fields
    {:ok, _coll} =
      DDL.create_collection("oauth_users",
        type: "auth",
        fields: [
          %{"name" => "email", "type" => "email", "required" => true, "indexed" => false},
          %{"name" => "password", "type" => "password", "required" => true, "indexed" => false}
        ]
      )

    Registry.reload!()

    # Register mock provider via settings
    Lazypock.Settings.put(%{
      "oauth2" => %{
        "providers" => [
          %{
            "name" => "mock",
            "config" => %{
              "client_id" => "mock-client",
              "client_secret" => "mock-secret",
              "strategy" => MockStrategy,
              "redirect_uri" => "http://localhost:4000/api/oauth2-redirect"
            }
          }
        ]
      }
    })

    on_exit(fn -> Lazypock.Settings.put(%{}) end)

    :ok
  end

  describe "GET /api/oauth_users/auth-methods" do
    test "returns configured oauth2 providers" do
      conn = get(build_conn(), "/api/oauth_users/auth-methods")
      body = json_response(conn, 200)

      assert body["password"] == true
      assert [provider] = body["oauth2"]["providers"]
      assert provider["name"] == "mock"
      assert provider["authURL"] =~ "example.test"
      assert provider["state"] != nil
      assert provider["codeVerifier"] != nil
    end
  end

  describe "POST /api/oauth_users/auth-with-oauth2" do
    test "creates a record on first OAuth2 sign-in and links external auth" do
      conn =
        post(build_conn(), "/api/oauth_users/auth-with-oauth2", %{
          "provider" => "mock",
          "code" => "auth-code-123",
          "codeVerifier" => "verifier-xyz",
          "redirectUrl" => "http://localhost:4000/api/oauth2-redirect",
          "createData" => %{"name" => "Mock Name"}
        })

      body = json_response(conn, 200)

      assert body["token"] != nil
      assert body["record"]["email"] == "mock@example.com"
      assert body["meta"]["isNew"] == true
      assert body["meta"]["name"] == "Mock User"

      # External auth link created
      assert {:ok, link} = OAuth2.find_external_auth("oauth_users", "mock", "ext-user-123")
      assert link["collection"] == "oauth_users"
    end

    test "returns existing record on second sign-in (idempotent)" do
      # First sign-in
      conn =
        post(build_conn(), "/api/oauth_users/auth-with-oauth2", %{
          "provider" => "mock",
          "code" => "auth-code-123",
          "codeVerifier" => "verifier-xyz",
          "redirectUrl" => "http://localhost:4000/api/oauth2-redirect"
        })

      body1 = json_response(conn, 200)
      assert body1["meta"]["isNew"] == true

      # Second sign-in — same provider user
      conn2 =
        post(build_conn(), "/api/oauth_users/auth-with-oauth2", %{
          "provider" => "mock",
          "code" => "auth-code-456",
          "codeVerifier" => "verifier-abc",
          "redirectUrl" => "http://localhost:4000/api/oauth2-redirect"
        })

      body2 = json_response(conn2, 200)
      assert body2["meta"]["isNew"] == false
      assert body2["record"]["id"] == body1["record"]["id"]

      # Only one user record
      users = GenericRecord.all("oauth_users")
      assert length(users) == 1
    end

    test "returns 400 when provider missing" do
      conn =
        post(build_conn(), "/api/oauth_users/auth-with-oauth2", %{
          "code" => "auth-code-123",
          "codeVerifier" => "verifier-xyz",
          "redirectUrl" => "http://localhost:4000/api/oauth2-redirect"
        })

      assert json_response(conn, 400)["message"] =~ "provider"
    end

    test "returns 404 for unknown collection" do
      conn =
        post(build_conn(), "/api/nope/auth-with-oauth2", %{
          "provider" => "mock",
          "code" => "auth-code-123",
          "codeVerifier" => "verifier-xyz",
          "redirectUrl" => "http://localhost:4000/api/oauth2-redirect"
        })

      assert json_response(conn, 404)["message"] =~ "Collection not found"
    end
  end

  describe "GET /api/oauth2-redirect" do
    test "exchanges code via session state and returns postMessage HTML" do
      # Simulate auth-methods flow: store a session with state
      OAuth2.store_session("mock", "oauth_users", "verifier-xyz", "state-123")

      conn = get(build_conn(), "/api/oauth2-redirect?code=auth-code-999&state=state-123")
      assert conn.status == 200
      assert Plug.Conn.get_resp_header(conn, "content-type") |> List.first() =~ "text/html"
      assert conn.resp_body =~ "lazypock:oauth2"
      assert conn.resp_body =~ "mock@example.com"
    end

    test "rejects invalid session state" do
      conn = get(build_conn(), "/api/oauth2-redirect?code=auth-code-999&state=bogus")
      assert conn.status == 400
      assert conn.resp_body =~ "Invalid or expired OAuth2 session"
    end
  end
end
