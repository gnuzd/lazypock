defmodule LazypockWeb.SettingsApiKeyTest do
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Settings
  alias Lazypock.Schema.DDL
  alias Lazypock.Repo
  alias Lazypock.Auth.SuperUser
  alias Lazypock.Auth.Token

  setup do
    {:ok, _} = DDL.create_collection("test_coll", type: "base", fields: [])
    {:ok, conn: build_conn()}
  end

  defp auth_conn(conn) do
    superuser = %SuperUser{
      id: Ecto.UUID.generate(),
      email: "admin@test.com",
      password_hash: Bcrypt.hash_pwd_salt("password")
    }

    Repo.insert!(superuser)
    {:ok, token} = Token.generate_access_token(superuser)
    put_req_header(conn, "authorization", "Bearer #{token}")
  end

  test "generates an API key requiring superuser" do
    conn = post(auth_conn(build_conn()), "/api/settings/api-key")
    body = json_response(conn, 200)
    assert body["api_key"] =~ "lazypock_"
    # Only the hash is persisted; the raw key is not recoverable.
    assert Settings.get("api_key") == Settings.hash_key(body["api_key"])
  end

  test "returns has_api_key=true when one exists (raw key never returned)" do
    Settings.set_api_key("lazypock_" <> String.duplicate("a", 32))
    conn = get(auth_conn(build_conn()), "/api/settings/api-key")
    body = json_response(conn, 200)
    assert body["has_api_key"] == true
    # The endpoint must never leak the raw key (nor a masked version).
    assert body["api_key"] == nil
  end

  test "returns has_api_key=false when none exists" do
    conn = get(auth_conn(build_conn()), "/api/settings/api-key")
    body = json_response(conn, 200)
    assert body["has_api_key"] == false
    assert body["api_key"] == nil
  end

  test "revoke removes the key" do
    Settings.set_api_key("lazypock_secret")
    conn = delete(auth_conn(build_conn()), "/api/settings/api-key")
    assert json_response(conn, 200)["has_api_key"] == false
    assert Settings.get("api_key") == nil
  end

  test "API key can list collections (codegen path)" do
    key = Settings.new_api_key()
    Settings.set_api_key(key)

    conn =
      build_conn()
      |> put_req_header("authorization", "Bearer #{key}")
      |> get("/api/collections")

    assert response(conn, 200)
  end

  test "API key CANNOT create collections (scoped to GET /collections only)" do
    key = Settings.new_api_key()
    Settings.set_api_key(key)

    conn =
      build_conn()
      |> put_req_header("authorization", "Bearer #{key}")
      |> post("/api/collections", %{"name" => "evil_coll"})

    assert response(conn, 403)
  end

  test "API key cannot access superuser-only settings (no superuser bypass)" do
    key = Settings.new_api_key()
    Settings.set_api_key(key)

    conn =
      build_conn()
      |> put_req_header("authorization", "Bearer #{key}")
      |> get("/api/settings")

    assert response(conn, 403)
  end

  test "invalid API key cannot list collections" do
    conn =
      build_conn()
      |> put_req_header("authorization", "Bearer wrongkey")
      |> get("/api/collections")

    assert response(conn, 403)
  end

  test "unauth cannot access api-key endpoints" do
    assert response(get(build_conn(), "/api/settings/api-key"), 403)
  end
end
