defmodule Lazypock.Auth.OAuth2Test do
  use Lazypock.DataCase, async: false

  alias Lazypock.Auth.OAuth2
  alias Lazypock.Schemas.GenericRecord

  # A mock Assent strategy that returns fixed user data without hitting the network.
  defmodule MockStrategy do
    @moduledoc false
    use Assent.Strategy.OAuth2.Base

    @impl true
    def default_config(_config) do
      [
        base_url: "https://example.test",
        authorize_url: "https://example.test/oauth/authorize",
        token_url: "https://example.test/oauth/token",
        user_url: "https://example.test/oauth/user",
        authorization_params: []
      ]
    end

    @impl true
    def normalize(_config, user) do
      {:ok,
       %{
         "sub" => user["id"],
         "name" => user["name"],
         "email" => user["email"],
         "picture" => user["avatar_url"]
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
  end

  setup do
    # Ensure the external auths table exists (boot normally does this)
    OAuth2.ensure_external_auths_table!()
    OAuth2.ensure_session_table!()

    # Register a mock provider via runtime settings
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

  describe "providers/0" do
    test "returns configured providers from runtime settings" do
      providers = OAuth2.providers()
      assert {"mock", cfg} = List.first(providers)
      assert cfg[:client_id] == "mock-client"
    end

    test "provider_config/1 returns config for known provider" do
      cfg = OAuth2.provider_config("mock")
      assert cfg[:client_id] == "mock-client"
    end

    test "provider_config/1 returns nil for unknown provider" do
      assert OAuth2.provider_config("nope") == nil
    end
  end

  describe "authorize_url/1" do
    test "builds an authorization URL with state + PKCE" do
      assert {:ok, %{url: url, session_params: session_params}} = OAuth2.authorize_url("mock")
      assert url =~ "https://example.test/oauth/authorize"
      assert url =~ "client_id=mock-client"
      assert url =~ "code_challenge"
      assert session_params[:state] != nil
      assert session_params[:code_verifier] != nil
    end

    test "returns error for unknown provider" do
      assert {:error, :unknown_provider} = OAuth2.authorize_url("nope")
    end
  end

  describe "session store" do
    test "store_session + take_session roundtrip" do
      OAuth2.store_session("mock", "users", "verifier-123", "state-abc")
      assert {:ok, "mock", "users", "verifier-123"} = OAuth2.take_session("state-abc")
      # consumed once
      assert {:error, :not_found} = OAuth2.take_session("state-abc")
    end
  end

  describe "external auth linking" do
    test "find_external_auth returns not_found when absent" do
      assert {:error, :not_found} = OAuth2.find_external_auth("users", "mock", "ext-1")
    end

    test "find_or_create_external_auth creates then finds" do
      user_id = Ecto.UUID.generate()

      assert {:ok, link} =
               OAuth2.find_or_create_external_auth("users", "mock", "ext-1", user_id)

      assert link["provider"] == "mock"
      assert link["provider_id"] == "ext-1"
      assert link["collection"] == "users"

      assert {:ok, link2} = OAuth2.find_external_auth("users", "mock", "ext-1")
      assert link2["user_id"] == user_id
    end

    test "find_or_create_external_auth is idempotent (unique constraint)" do
      user_id = Ecto.UUID.generate()
      {:ok, _} = OAuth2.find_or_create_external_auth("users", "mock", "ext-2", user_id)
      {:ok, _} = OAuth2.find_or_create_external_auth("users", "mock", "ext-2", user_id)
      links = GenericRecord.all("_external_auths")
      assert length(links) == 1
    end
  end
end
