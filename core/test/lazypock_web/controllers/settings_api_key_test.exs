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

  test "creates an API key with optional expiry" do
    conn = post(auth_conn(build_conn()), "/api/settings/api-keys", %{"expiresInDays" => 30})
    body = json_response(conn, 200)
    assert body["api_key"] =~ "lazypock_"
    assert body["expires_at"] != nil
    assert body["item"]["id"]
  end

  test "creates a non-expiring API key by default" do
    conn = post(auth_conn(build_conn()), "/api/settings/api-keys")
    body = json_response(conn, 200)
    assert body["api_key"] =~ "lazypock_"
    assert body["expires_at"] == nil
  end

  test "lists API keys (metadata only, never the raw key)" do
    {raw, _} = Settings.create_api_key()
    conn = get(auth_conn(build_conn()), "/api/settings/api-keys")
    body = json_response(conn, 200)

    [item] = body["items"]
    assert item["id"]
    refute item["hash"]
    # Raw key must never leak.
    refute Jason.encode!(body) =~ raw
  end

  test "revokes an API key by id" do
    {key, meta} = Settings.create_api_key()
    conn = delete(auth_conn(build_conn()), "/api/settings/api-keys/" <> meta["id"])
    assert json_response(conn, 200)["ok"] == true
    assert Settings.verify_api_key(key) == false
  end

  test "revoking unknown id returns 404" do
    conn = delete(auth_conn(build_conn()), "/api/settings/api-keys/nope")
    assert response(conn, 404)
  end

  test "API key can list collections (codegen path)" do
    {key, _} = Settings.create_api_key()

    conn =
      build_conn()
      |> put_req_header("authorization", "Bearer #{key}")
      |> get("/api/collections")

    assert response(conn, 200)
  end

  test "expired API key cannot list collections" do
    {key, meta} = Settings.create_api_key()
    now = DateTime.utc_now()
    past = now |> DateTime.add(-3600, :second) |> DateTime.to_iso8601()

    Settings.put(%{
      "api_keys" => [
        %{
          "id" => meta["id"],
          "hash" => Settings.hash_key(key),
          "created_at" => DateTime.to_iso8601(now),
          "expires_at" => past,
          "revoked" => false
        }
      ]
    })

    conn =
      build_conn()
      |> put_req_header("authorization", "Bearer #{key}")
      |> get("/api/collections")

    assert response(conn, 403)
  end

  test "revoked API key cannot list collections" do
    {key, meta} = Settings.create_api_key()
    Settings.revoke_api_key(meta["id"])

    conn =
      build_conn()
      |> put_req_header("authorization", "Bearer #{key}")
      |> get("/api/collections")

    assert response(conn, 403)
  end

  test "API key CANNOT create collections (scoped to GET /collections only)" do
    {key, _} = Settings.create_api_key()

    conn =
      build_conn()
      |> put_req_header("authorization", "Bearer #{key}")
      |> post("/api/collections", %{"name" => "evil_coll"})

    assert response(conn, 403)
  end

  test "API key cannot access superuser-only settings (no superuser bypass)" do
    {key, _} = Settings.create_api_key()

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
    assert response(get(build_conn(), "/api/settings/api-keys"), 403)
  end
end
