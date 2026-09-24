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

  describe "tampering and forgery" do
    # Tokens are Phoenix.Token (HMAC over the endpoint's secret_key_base), so
    # there is no JWT `alg` header to confuse and no `nbf` claim. The relevant
    # attack classes are: expired, re-signed with another key, and claims that
    # verify but carry nothing usable.

    test "an expired superuser token is rejected" do
      expired =
        sign("superuser", %{"type" => "superuser", "id" => Ecto.UUID.generate()},
          signed_at: System.system_time(:second) - 8 * 24 * 60 * 60
        )

      assert {:error, _reason} = Token.verify_token(expired)
    end

    test "an expired user token is rejected" do
      expired =
        sign(
          "auth_user",
          %{"type" => "user", "id" => Ecto.UUID.generate(), "collectionName" => "accounts"},
          signed_at: System.system_time(:second) - 8 * 24 * 60 * 60
        )

      assert {:error, _reason} = Token.verify_user_token(expired)
    end

    test "a token signed with the wrong key is rejected" do
      forged =
        Phoenix.Token.sign(
          :crypto.strong_rand_bytes(64),
          "superuser",
          %{"type" => "superuser", "id" => Ecto.UUID.generate()}
        )

      assert {:error, _reason} = Token.verify_token(forged)
    end

    test "a tampered payload is rejected" do
      superuser = %SuperUser{id: Ecto.UUID.generate(), email: "admin@test.com"}
      {:ok, token} = Token.generate_access_token(superuser)

      tampered = tamper(token)

      assert tampered != token
      assert {:error, _reason} = Token.verify_token(tampered)
    end

    test "a token with no id claim verifies but carries no identity" do
      # verify_token/1 intentionally only checks the type claim; callers must
      # treat a missing id as unauthenticated (Auth.Plug looks the id up and
      # assigns nil when it is absent).
      {:ok, claims} = Token.verify_token(sign("superuser", %{"type" => "superuser"}))
      assert claims["id"] == nil

      {:ok, user_claims} =
        Token.verify_user_token(
          sign("auth_user", %{"type" => "user", "collectionName" => "accounts"})
        )

      assert user_claims["id"] == nil
    end

    test "a token with the wrong type claim is rejected" do
      assert {:error, "Invalid token type"} =
               Token.verify_token(sign("superuser", %{"type" => "user", "id" => "x"}))

      assert {:error, "Invalid token type"} =
               Token.verify_user_token(sign("auth_user", %{"type" => "superuser", "id" => "x"}))
    end

    test "a token with no type claim is rejected" do
      assert {:error, _} = Token.verify_token(sign("superuser", %{"id" => "x"}))
      assert {:error, _} = Token.verify_user_token(sign("auth_user", %{"id" => "x"}))
    end
  end

  defp sign(salt, data, opts \\ []) do
    Phoenix.Token.sign(LazypockWeb.Endpoint, salt, data, opts)
  end

  # Flip a character in the payload segment, leaving the token well-formed.
  defp tamper(token) do
    [payload | rest] = String.split(token, ".", parts: 3)
    <<first, tail::binary>> = payload
    flipped = if first == ?a, do: "b", else: "a"
    Enum.join([flipped <> tail | rest], ".")
  end
end
