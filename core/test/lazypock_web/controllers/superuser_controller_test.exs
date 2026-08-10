defmodule LazypockWeb.SuperUserControllerTest do
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Auth.SuperUser
  alias Lazypock.Auth.Token
  alias Lazypock.Repo
  alias Lazypock.Schema.DDL
  alias Lazypock.Schemas.GenericRecord
  alias Lazypock.Collections.Registry

  defp create_superuser(email \\ "admin@test.com", password \\ "password123") do
    %SuperUser{
      id: Ecto.UUID.generate(),
      email: email,
      password_hash: Bcrypt.hash_pwd_salt(password)
    }
    |> Repo.insert!()
  end

  defp superuser_conn(conn, superuser) do
    {:ok, token} = Token.generate_access_token(superuser)
    put_req_header(conn, "authorization", "Bearer #{token}")
  end

  describe "GET /api/me" do
    test "returns the superuser identity for a superuser token" do
      superuser = create_superuser()

      conn = build_conn() |> superuser_conn(superuser) |> get("/api/me")
      body = json_response(conn, 200)

      assert body["id"] == Ecto.UUID.cast!(superuser.id)
      assert body["email"] == "admin@test.com"
      assert body["collectionName"] == "_superusers"
    end

    test "returns 401 without a token" do
      conn = get(build_conn(), "/api/me")
      assert json_response(conn, 401)["message"] == "Not authenticated"
    end

    test "returns the auth collection user record for a user token" do
      # Create an auth collection with a user
      {:ok, _coll} =
        DDL.create_collection("me_test_users",
          type: "auth",
          fields: [
            %{"name" => "email", "type" => "email", "required" => true, "indexed" => false},
            %{"name" => "password", "type" => "password", "required" => true, "indexed" => false}
          ]
        )

      Registry.reload!()

      {:ok, user} =
        GenericRecord.insert("me_test_users", %{
          "email" => "alice@test.com",
          "password" => "secret123"
        })

      {:ok, token} = Token.generate_user_token(user, "me_test_users")

      conn =
        build_conn()
        |> put_req_header("authorization", "Bearer #{token}")
        |> get("/api/me")

      body = json_response(conn, 200)
      assert body["email"] == "alice@test.com"
      assert body["id"] == user["id"]
    end
  end

  describe "GET /api/superusers/me (legacy alias)" do
    test "still returns the superuser identity" do
      superuser = create_superuser()

      conn = build_conn() |> superuser_conn(superuser) |> get("/api/superusers/me")
      body = json_response(conn, 200)

      assert body["email"] == "admin@test.com"
    end
  end

  describe "POST /api/_superusers/auth-with-password (PocketBase parity)" do
    test "logs in a superuser and returns a token" do
      superuser = create_superuser()

      conn =
        post(build_conn(), "/api/_superusers/auth-with-password", %{
          "identity" => "admin@test.com",
          "password" => "password123"
        })

      body = json_response(conn, 200)
      assert body["token"] != nil
      assert body["record"]["email"] == "admin@test.com"
      assert body["record"]["id"] == Ecto.UUID.cast!(superuser.id)

      # Token actually works
      {:ok, claims} = Token.verify_token(body["token"])
      assert claims["id"] == Ecto.UUID.cast!(superuser.id)
    end

    test "returns 401 for wrong password" do
      create_superuser()

      conn =
        post(build_conn(), "/api/_superusers/auth-with-password", %{
          "identity" => "admin@test.com",
          "password" => "wrong"
        })

      assert json_response(conn, 401)["message"] == "Invalid email or password"
    end

    test "returns 400 when identity missing" do
      conn =
        post(build_conn(), "/api/_superusers/auth-with-password", %{
          "password" => "password123"
        })

      assert json_response(conn, 400)["message"] =~ "identity"
    end
  end

  describe "GET /api/_superusers/auth-methods" do
    test "returns password auth enabled" do
      conn = get(build_conn(), "/api/_superusers/auth-methods")
      body = json_response(conn, 200)
      assert body["password"] == true
      assert body["oauth2"]["providers"] == []
    end
  end
end
