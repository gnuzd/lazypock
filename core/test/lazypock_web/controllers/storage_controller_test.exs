defmodule LazypockWeb.StorageControllerTest do
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Auth.SuperUser
  alias Lazypock.Auth.Token
  alias Lazypock.Files.Storage
  alias Lazypock.Files.Storage.Secret
  alias Lazypock.Repo
  alias Lazypock.Settings

  @stub :lazypock_storage_stub

  defp superuser_token do
    su = %SuperUser{
      id: Ecto.UUID.generate(),
      email: "admin#{System.unique_integer([:positive])}@test.com",
      password_hash: Bcrypt.hash_pwd_salt("password")
    }

    Repo.insert!(su)
    {:ok, token} = Token.generate_access_token(su)
    token
  end

  defp auth_conn(conn), do: put_req_header(conn, "authorization", "Bearer #{superuser_token()}")

  setup do
    original = Settings.get()
    Storage.clear_cache()

    on_exit(fn ->
      Settings.put(original)
      Storage.clear_cache()
      Application.delete_env(:lazypock, Lazypock.Files.Adapters.S3)
    end)

    :ok
  end

  test "requires a superuser" do
    assert response(get(build_conn(), "/api/settings/storage"), 403)
    assert response(patch(build_conn(), "/api/settings/storage", %{"backend" => "local"}), 403)
  end

  test "stores the s3 config, encrypts the secret and masks it on read" do
    conn =
      auth_conn(build_conn())
      |> put_req_header("content-type", "application/json")
      |> patch("/api/settings/storage", %{
        "backend" => "s3",
        "endpoint" => "https://account.r2.cloudflarestorage.com",
        "bucket" => "media",
        "access_key_id" => "key",
        "secret_access_key" => "super-secret"
      })

    body = json_response(conn, 200)
    assert body["backend"] == "s3"
    assert body["secret_access_key"] == Storage.mask()
    assert body["secret_set"] == true

    # Persisted encrypted, never plaintext.
    stored = Settings.get("storage")
    assert String.starts_with?(stored["secret_access_key"], "enc:")
    assert {:ok, "super-secret"} = Secret.decrypt(stored["secret_access_key"])

    # Reading again still masks it.
    refute json_response(auth_conn(build_conn()) |> get("/api/settings/storage"), 200)[
             "secret_access_key"
           ] =~ "super-secret"
  end

  test "rejects an incomplete s3 config with 422" do
    conn =
      auth_conn(build_conn())
      |> put_req_header("content-type", "application/json")
      |> patch("/api/settings/storage", %{"backend" => "s3", "bucket" => "media"})

    assert json_response(conn, 422)["message"] =~ "endpoint"
  end

  test "test endpoint reports each round-trip step" do
    Application.put_env(:lazypock, Lazypock.Files.Adapters.S3,
      req_options: [plug: {Req.Test, @stub}]
    )

    Req.Test.stub(@stub, fn conn ->
      case conn.method do
        "PUT" -> Plug.Conn.send_resp(conn, 200, "")
        "HEAD" -> Plug.Conn.send_resp(conn, 200, "")
        "GET" -> Plug.Conn.send_resp(conn, 200, "lazypock")
        "DELETE" -> Plug.Conn.send_resp(conn, 204, "")
      end
    end)

    Settings.put(
      Map.put(Settings.get(), "storage", %{
        "backend" => "s3",
        "endpoint" => "https://s3.test",
        "bucket" => "bucket",
        "access_key_id" => "k",
        "secret_access_key" => "s"
      })
    )

    Storage.clear_cache()

    body = json_response(auth_conn(build_conn()) |> post("/api/settings/storage/test", %{}), 200)
    assert body["ok"] == true
    assert Enum.map(body["steps"], & &1["step"]) == ["put", "head", "get", "delete"]
  end

  test "test endpoint surfaces a failing step" do
    Application.put_env(:lazypock, Lazypock.Files.Adapters.S3,
      req_options: [plug: {Req.Test, @stub}]
    )

    Req.Test.stub(@stub, fn conn -> Plug.Conn.send_resp(conn, 403, "AccessDenied") end)

    Settings.put(
      Map.put(Settings.get(), "storage", %{
        "backend" => "s3",
        "endpoint" => "https://s3.test",
        "bucket" => "bucket",
        "access_key_id" => "k",
        "secret_access_key" => "s"
      })
    )

    Storage.clear_cache()

    body = json_response(auth_conn(build_conn()) |> post("/api/settings/storage/test", %{}), 422)
    refute body["ok"]
    assert Enum.any?(body["steps"], &(not &1["ok"]))
  end
end
