defmodule LazypockWeb.AuthFlowTest do
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Schema.DDL
  alias Lazypock.Schemas.GenericRecord
  alias Lazypock.Collections.Registry

  @moduledoc """
  End-to-end auth collection flows: auth-with-password, auth-refresh,
  auth-methods, and rule enforcement on user JWTs.
  """

  defp cname(prefix), do: "#{prefix}_#{System.unique_integer([:positive]) |> abs()}"

  defp create_auth_collection(name) do
    {:ok, _coll} =
      DDL.create_collection(name,
        type: "auth",
        fields: [
          %{"name" => "email", "type" => "email", "required" => true},
          %{"name" => "password_hash", "type" => "password", "required" => true},
          %{"name" => "name", "type" => "text"}
        ]
      )

    Registry.reload!()
    :ok
  end

  defp create_user(collection, email, password, extra \\ %{}) do
    {:ok, user} =
      GenericRecord.insert(
        collection,
        Map.merge(
          %{
            "email" => email,
            "password_hash" => Bcrypt.hash_pwd_salt(password)
          },
          extra
        )
      )

    user
  end

  describe "POST /api/:collection/auth-with-password" do
    test "returns a token and the user record for valid credentials" do
      name = cname("auth_pw")
      create_auth_collection(name)
      create_user(name, "alice@test.com", "secret123")

      conn =
        post(build_conn(), "/api/#{name}/auth-with-password", %{
          "identity" => "alice@test.com",
          "password" => "secret123"
        })

      body = json_response(conn, 200)
      assert body["token"] != nil
      assert body["record"]["email"] == "alice@test.com"
      # Password hash never leaks
      refute Map.has_key?(body["record"], "password_hash")
    end

    test "supports identity param as the login key" do
      name = cname("auth_pw_id")
      create_auth_collection(name)
      create_user(name, "bob@test.com", "secret123")

      conn =
        post(build_conn(), "/api/#{name}/auth-with-password", %{
          "identity" => "bob@test.com",
          "password" => "secret123"
        })

      assert json_response(conn, 200)["record"]["email"] == "bob@test.com"
    end

    test "returns 401 for wrong password" do
      name = cname("auth_badpw")
      create_auth_collection(name)
      create_user(name, "alice@test.com", "secret123")

      conn =
        post(build_conn(), "/api/#{name}/auth-with-password", %{
          "identity" => "alice@test.com",
          "password" => "wrong"
        })

      body = json_response(conn, 401)
      assert body["message"] =~ "Invalid email or password"
    end

    test "returns 401 for unknown email" do
      name = cname("auth_noemail")
      create_auth_collection(name)

      conn =
        post(build_conn(), "/api/#{name}/auth-with-password", %{
          "identity" => "ghost@test.com",
          "password" => "x"
        })

      assert json_response(conn, 401)["message"] =~ "Invalid email or password"
    end

    test "returns 400 when identity or password missing" do
      name = cname("auth_missing")
      create_auth_collection(name)

      conn = post(build_conn(), "/api/#{name}/auth-with-password", %{"password" => "x"})
      assert json_response(conn, 400)["message"] =~ "identity or email"

      conn = post(build_conn(), "/api/#{name}/auth-with-password", %{"identity" => "a@b.c"})
      assert json_response(conn, 400)["message"] =~ "password"
    end

    test "returns 400 for a non-auth collection" do
      name = cname("notauth")
      {:ok, _} = DDL.create_collection(name, type: "base", fields: [])
      Registry.reload!()

      conn =
        post(build_conn(), "/api/#{name}/auth-with-password", %{
          "identity" => "a@b.c",
          "password" => "x"
        })

      assert json_response(conn, 400)["message"] =~ "Not an auth collection"
    end

    test "returns 404 for an unknown collection" do
      conn =
        post(build_conn(), "/api/does_not_exist/auth-with-password", %{
          "identity" => "a@b.c",
          "password" => "x"
        })

      assert json_response(conn, 404)["message"] =~ "not found"
    end
  end

  describe "user registration via REST API (POST /api/users)" do
    test "creates a user with `password` (PocketBase-style) and hashes it" do
      conn =
        post(build_conn(), "/api/users", %{
          "data" => %{"email" => "reg@test.com", "password" => "secret123"}
        })

      body = json_response(conn, 201)
      assert body["email"] == "reg@test.com"
      assert body["collectionName"] == "users"
      # Password must never leak in responses
      refute Map.has_key?(body, "password")
      refute Map.has_key?(body, "password_hash")

      # The stored value is a bcrypt hash, not the plaintext
      {:ok, coll} = Registry.get("users")

      password_field =
        Enum.find(coll.fields, &(&1.type == "password"))

      [user] = GenericRecord.all_where("users", "email = $1", ["reg@test.com"])
      stored = user[password_field.name]
      assert is_binary(stored)
      refute stored == "secret123"
      assert Bcrypt.verify_pass("secret123", stored)
    end

    test "legacy password_hash key still works (backward compat)" do
      conn =
        post(build_conn(), "/api/users", %{
          "data" => %{"email" => "legacy@test.com", "password_hash" => "hunter22"}
        })

      assert json_response(conn, 201)["email"] == "legacy@test.com"

      conn =
        post(build_conn(), "/api/users/auth-with-password", %{
          "identity" => "legacy@test.com",
          "password" => "hunter22"
        })

      assert json_response(conn, 200)["token"] != nil
    end

    test "created user can log in via auth-with-password" do
      post(build_conn(), "/api/users", %{
        "data" => %{"email" => "reg2@test.com", "password" => "secret123"}
      })

      conn =
        post(build_conn(), "/api/users/auth-with-password", %{
          "identity" => "reg2@test.com",
          "password" => "secret123"
        })

      body = json_response(conn, 200)
      assert body["token"] != nil
      assert body["record"]["email"] == "reg2@test.com"
      refute Map.has_key?(body["record"], "password_hash")
    end

    test "password is NOT required — user can be created without one" do
      conn =
        post(build_conn(), "/api/users", %{
          "data" => %{"email" => "reg3@test.com"}
        })

      body = json_response(conn, 201)
      assert body["email"] == "reg3@test.com"
      refute Map.has_key?(body, "password_hash")

      # Record exists with a nil password hash
      {:ok, coll} = Registry.get("users")

      password_field =
        Enum.find(coll.fields, &(&1.type == "password"))

      [user] = GenericRecord.all_where("users", "email = $1", ["reg3@test.com"])
      assert is_nil(user[password_field.name])

      # And can't log in with a password
      conn =
        post(build_conn(), "/api/users/auth-with-password", %{
          "identity" => "reg3@test.com",
          "password" => "anything"
        })

      assert json_response(conn, 401)
    end

    test "users collection schema marks the password field hidden + non-required" do
      {:ok, coll} = Registry.get("users")

      password_field =
        Enum.find(coll.fields, &(&1.type == "password"))

      assert password_field.hidden == true
      assert password_field.required == false
    end
  end

  describe "POST /api/:collection/auth-refresh" do
    setup do
      name = cname("auth_refresh")
      create_auth_collection(name)
      create_user(name, "carol@test.com", "secret123")

      conn =
        post(build_conn(), "/api/#{name}/auth-with-password", %{
          "identity" => "carol@test.com",
          "password" => "secret123"
        })

      token = json_response(conn, 200)["token"]
      %{name: name, token: token}
    end

    test "returns a fresh token for a valid user JWT", %{name: name, token: token} do
      conn =
        build_conn()
        |> put_req_header("authorization", "Bearer #{token}")
        |> post("/api/#{name}/auth-refresh")

      body = json_response(conn, 200)
      assert body["token"] != nil
      assert body["record"]["email"] == "carol@test.com"
    end

    test "returns 401 without a token", %{name: name} do
      conn = post(build_conn(), "/api/#{name}/auth-refresh")
      assert json_response(conn, 401)["message"] =~ "Not authenticated"
    end

    test "returns 403 when the token belongs to another collection", %{token: token} do
      other = cname("auth_other")
      create_auth_collection(other)
      create_user(other, "dave@test.com", "secret123")

      conn =
        build_conn()
        |> put_req_header("authorization", "Bearer #{token}")
        |> post("/api/#{other}/auth-refresh")

      assert json_response(conn, 403)["message"] =~ "does not match this collection"
    end
  end

  describe "GET /api/:collection/auth-methods" do
    test "returns password auth for an auth collection" do
      name = cname("auth_methods")
      create_auth_collection(name)

      conn = get(build_conn(), "/api/#{name}/auth-methods")
      body = json_response(conn, 200)
      assert body["password"] == true
      assert body["oauth2"] == %{"providers" => []}
      assert body["mfa"] == %{}
    end

    test "returns 400 for a non-auth collection" do
      name = cname("auth_methods_base")
      {:ok, _} = DDL.create_collection(name, type: "base", fields: [])
      Registry.reload!()

      conn = get(build_conn(), "/api/#{name}/auth-methods")
      assert json_response(conn, 400)["message"] =~ "Not an auth collection"
    end
  end

  describe "rule enforcement on user JWTs" do
    test "user can update/delete only their own record (default auth rules)" do
      name = cname("auth_rules")
      create_auth_collection(name)
      me = create_user(name, "me@test.com", "secret123")
      other = create_user(name, "other@test.com", "secret123")

      conn =
        post(build_conn(), "/api/#{name}/auth-with-password", %{
          "identity" => "me@test.com",
          "password" => "secret123"
        })

      token = json_response(conn, 200)["token"]
      auth = put_req_header(build_conn(), "authorization", "Bearer #{token}")

      # Update own record → 200
      conn = patch(auth, "/api/#{name}/#{me["id"]}", %{"name" => "Me Updated"})
      assert json_response(conn, 200)["name"] == "Me Updated"

      # Update someone else's record → 403 (updateRule: id = @request.auth.id)
      conn = patch(auth, "/api/#{name}/#{other["id"]}", %{"name" => "Hacked"})
      assert response(conn, 403)

      # Delete someone else's record → 403
      conn = delete(auth, "/api/#{name}/#{other["id"]}")
      assert response(conn, 403)

      # Delete own record → 204
      conn = delete(auth, "/api/#{name}/#{me["id"]}")
      assert response(conn, 204)
    end

    test "unauthenticated user is denied updates by default (createRule requires auth)" do
      name = cname("auth_rule_unauth")
      create_auth_collection(name)
      user = create_user(name, "x@test.com", "secret123")

      conn = patch(build_conn(), "/api/#{name}/#{user["id"]}", %{"name" => "x"})
      assert response(conn, 403)
    end
  end
end
