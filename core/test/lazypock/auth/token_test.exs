defmodule Lazypock.Auth.TokenTest do
  use ExUnit.Case, async: true

  alias Lazypock.Auth.Token
  alias Lazypock.Auth.SuperUser

  @moduledoc """
  Tests for Lazypock.Auth.Token — superuser and auth collection user token generation/verification.

  These are pure logic tests (no DB), using Phoenix.Token which derives
  its signing key from the endpoint's secret_key_base at compile time.
  """

  describe "generate_access_token/1" do
    test "returns {:ok, token} for a valid superuser" do
      superuser = %SuperUser{id: Ecto.UUID.generate(), email: "admin@test.com"}
      assert {:ok, token} = Token.generate_access_token(superuser)
      assert is_binary(token)
      assert String.contains?(token, ".")
    end
  end

  describe "verify_token/1" do
    test "verifies a valid superuser token" do
      superuser = %SuperUser{id: Ecto.UUID.generate(), email: "admin@test.com"}
      {:ok, token} = Token.generate_access_token(superuser)

      assert {:ok, claims} = Token.verify_token(token)
      assert claims["id"] == superuser.id
      assert claims["email"] == superuser.email
      assert claims["type"] == "superuser"
    end

    test "rejects a user token" do
      {:ok, user_token} =
        Token.generate_user_token(
          %{"id" => Ecto.UUID.generate(), "email" => "user@test.com"},
          "accounts"
        )

      assert {:error, _reason} = Token.verify_token(user_token)
    end

    test "rejects an invalid token" do
      assert {:error, _reason} = Token.verify_token("not-a-valid-token")
    end

    test "rejects a malformed binary" do
      assert {:error, _reason} = Token.verify_token("")
    end
  end

  describe "generate_user_token/2" do
    test "returns {:ok, token} for a valid auth collection user" do
      record = %{"id" => Ecto.UUID.generate(), "email" => "alice@test.com"}
      assert {:ok, token} = Token.generate_user_token(record, "accounts")
      assert is_binary(token)
      assert String.contains?(token, ".")
    end

    test "includes collectionName and type claims" do
      record = %{"id" => Ecto.UUID.generate(), "email" => "alice@test.com"}
      {:ok, token} = Token.generate_user_token(record, "accounts")

      # Verify by round-tripping through verify_user_token
      {:ok, claims} = Token.verify_user_token(token)
      assert claims["id"] == record["id"]
      assert claims["email"] == record["email"]
      assert claims["collectionName"] == "accounts"
      assert claims["type"] == "user"
    end
  end

  describe "verify_user_token/1" do
    test "verifies a valid user token" do
      record = %{"id" => Ecto.UUID.generate(), "email" => "bob@test.com"}
      {:ok, token} = Token.generate_user_token(record, "users")

      assert {:ok, claims} = Token.verify_user_token(token)
      assert claims["id"] == record["id"]
      assert claims["email"] == record["email"]
      assert claims["collectionName"] == "users"
      assert claims["type"] == "user"
    end

    test "rejects a superuser token" do
      superuser = %SuperUser{id: Ecto.UUID.generate(), email: "admin@test.com"}
      {:ok, superuser_token} = Token.generate_access_token(superuser)

      assert {:error, _reason} = Token.verify_user_token(superuser_token)
    end

    test "rejects an invalid token" do
      assert {:error, _reason} = Token.verify_user_token("not-a-valid-token")
    end

    test "rejects a malformed binary" do
      assert {:error, _reason} = Token.verify_user_token("")
    end
  end

  describe "cross-contamination" do
    test "superuser token cannot be verified as user token" do
      superuser = %SuperUser{id: Ecto.UUID.generate(), email: "admin@test.com"}
      {:ok, token} = Token.generate_access_token(superuser)

      assert {:ok, _claims} = Token.verify_token(token)
      assert {:error, _reason} = Token.verify_user_token(token)
    end

    test "user token cannot be verified as superuser token" do
      record = %{"id" => Ecto.UUID.generate(), "email" => "bob@test.com"}
      {:ok, token} = Token.generate_user_token(record, "accounts")

      assert {:ok, _claims} = Token.verify_user_token(token)
      assert {:error, _reason} = Token.verify_token(token)
    end
  end
end
