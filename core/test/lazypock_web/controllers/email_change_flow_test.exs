defmodule LazypockWeb.EmailChangeFlowTest do
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Schema.DDL
  alias Lazypock.Schemas.GenericRecord
  alias Lazypock.Collections.Registry
  alias Lazypock.Auth.Token
  alias Lazypock.Repo

  @moduledoc """
  End-to-end email-change flow: request-email-change sends a token to the new
  address (captured via Swoosh.Adapters.Test) and confirm-email-change applies
  it after verifying the current password.
  """

  defp cname(prefix), do: "#{prefix}_#{System.unique_integer([:positive]) |> abs()}"

  defp create_auth_collection(name) do
    {:ok, _} =
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

  defp create_user(name, email, password) do
    {:ok, user} =
      GenericRecord.insert(name, %{
        "email" => email,
        "password_hash" => Bcrypt.hash_pwd_salt(password)
      })

    user
  end

  defp auth_conn(name, user) do
    {:ok, token} = Token.generate_user_token(user, name)
    put_req_header(build_conn(), "authorization", "Bearer #{token}")
  end

  defp request_change(name, user, new_email) do
    auth_conn(name, user)
    |> post("/api/#{name}/request-email-change", %{"newEmail" => new_email})
  end

  # Capture the raw change token delivered to the test process by
  # Swoosh.Adapters.Test, then return it plus the delivered email.
  defp capture_change_token do
    assert_receive {:email, %Swoosh.Email{} = email}
    [_, token] = Regex.run(~r/href="([A-Za-z0-9_-]+)"/, email.html_body || "")
    {token, email}
  end

  describe "POST /api/:collection/request-email-change" do
    test "sends a change email and records the new address on the OTP" do
      name = cname("ec_req")
      create_auth_collection(name)
      user = create_user(name, "old@test.com", "secret123")

      conn = request_change(name, user, "new@test.com")
      assert response(conn, 204)

      {_token, email} = capture_change_token()
      assert email.subject =~ "Confirm"
      assert email.html_body =~ "new@test.com"

      {:ok, %{rows: [[sent_to, record_ref]]}} =
        Ecto.Adapters.SQL.query(
          Repo,
          "SELECT sent_to, record_ref FROM _otps WHERE collection_ref = $1",
          [name]
        )

      assert sent_to == "new@test.com"
      assert record_ref == user["id"]
    end

    test "returns 401 when not authenticated" do
      name = cname("ec_req_anon")
      create_auth_collection(name)

      conn =
        post(build_conn(), "/api/#{name}/request-email-change", %{"newEmail" => "new@test.com"})

      assert json_response(conn, 401)["message"] =~ "Not authenticated"
    end

    test "returns 400 when newEmail is missing" do
      name = cname("ec_req_missing")
      create_auth_collection(name)
      user = create_user(name, "old@test.com", "secret123")

      conn = auth_conn(name, user) |> post("/api/#{name}/request-email-change", %{})
      assert json_response(conn, 400)["message"] =~ "newEmail"
    end

    test "returns 400 for a non-auth collection" do
      name = cname("ec_req_notauth")

      {:ok, _} =
        DDL.create_collection(name,
          type: "base",
          fields: [%{"name" => "email", "type" => "email"}]
        )

      Registry.reload!()
      {:ok, record} = GenericRecord.insert(name, %{"email" => "old@test.com"})

      conn =
        auth_conn(name, record)
        |> post("/api/#{name}/request-email-change", %{"newEmail" => "new@test.com"})

      assert json_response(conn, 400)["message"] =~ "Not an auth collection"
    end
  end

  describe "POST /api/:collection/confirm-email-change" do
    test "updates the email when token and password are valid" do
      name = cname("ec_confirm")
      create_auth_collection(name)
      user = create_user(name, "old@test.com", "secret123")

      assert response(request_change(name, user, "new@test.com"), 204)
      {token, _email} = capture_change_token()

      conn =
        auth_conn(name, user)
        |> post("/api/#{name}/confirm-email-change", %{
          "token" => token,
          "password" => "secret123"
        })

      body = json_response(conn, 200)
      assert body["record"]["email"] == "new@test.com"
      refute Map.has_key?(body["record"], "password_hash")

      # Database was updated
      updated = GenericRecord.get(name, user["id"])
      assert updated["email"] == "new@test.com"

      # OTP was consumed
      {:ok, %{num_rows: 0}} =
        Ecto.Adapters.SQL.query(Repo, "SELECT 1 FROM _otps WHERE collection_ref = $1", [name])
    end

    test "returns 400 with the wrong password" do
      name = cname("ec_badpw")
      create_auth_collection(name)
      user = create_user(name, "old@test.com", "secret123")

      assert response(request_change(name, user, "new@test.com"), 204)
      {token, _email} = capture_change_token()

      conn =
        auth_conn(name, user)
        |> post("/api/#{name}/confirm-email-change", %{"token" => token, "password" => "wrong"})

      assert json_response(conn, 400)["message"] =~ "Invalid password"
      # Email unchanged
      assert GenericRecord.get(name, user["id"])["email"] == "old@test.com"
    end

    test "returns 400 with an invalid token" do
      name = cname("ec_badtok")
      create_auth_collection(name)
      user = create_user(name, "old@test.com", "secret123")

      conn =
        auth_conn(name, user)
        |> post("/api/#{name}/confirm-email-change", %{
          "token" => "bogus-token",
          "password" => "secret123"
        })

      assert json_response(conn, 400)["message"] =~ "Invalid"
      assert GenericRecord.get(name, user["id"])["email"] == "old@test.com"
    end

    test "rejects a token issued for a different user" do
      name = cname("ec_other_user")
      create_auth_collection(name)
      alice = create_user(name, "alice@test.com", "secret123")
      bob = create_user(name, "bob@test.com", "secret123")

      # Alice requests a change, then Bob tries to consume her token.
      assert response(request_change(name, alice, "alice-new@test.com"), 204)
      {token, _email} = capture_change_token()

      conn =
        auth_conn(name, bob)
        |> post("/api/#{name}/confirm-email-change", %{
          "token" => token,
          "password" => "secret123"
        })

      assert json_response(conn, 400)["message"] =~ "Invalid"
      assert GenericRecord.get(name, alice["id"])["email"] == "alice@test.com"
    end

    test "returns 401 when not authenticated" do
      name = cname("ec_confirm_anon")
      create_auth_collection(name)

      conn =
        post(build_conn(), "/api/#{name}/confirm-email-change", %{
          "token" => "x",
          "password" => "y"
        })

      assert json_response(conn, 401)["message"] =~ "Not authenticated"
    end
  end
end
