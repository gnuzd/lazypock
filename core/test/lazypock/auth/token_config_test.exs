defmodule Lazypock.Auth.TokenConfigTest do
  # async: false — these tests mutate process-global env vars that
  # Lazypock.Auth.Token reads at call time.
  use ExUnit.Case, async: false

  alias Lazypock.Auth.SuperUser
  alias Lazypock.Auth.Token
  alias LazypockWeb.Endpoint

  @default_ttl 7 * 24 * 60 * 60

  setup do
    saved_ttl = System.get_env("LAZYPOCK_AUTH_TOKEN_TTL")
    saved_secret = System.get_env("LAZYPOCK_AUTH_TOKEN_SECRET")

    on_exit(fn ->
      restore("LAZYPOCK_AUTH_TOKEN_TTL", saved_ttl)
      restore("LAZYPOCK_AUTH_TOKEN_SECRET", saved_secret)
    end)

    :ok
  end

  defp restore(name, nil), do: System.delete_env(name)
  defp restore(name, value), do: System.put_env(name, value)

  defp superuser do
    %SuperUser{id: Ecto.UUID.generate(), email: "cfg@test.com"}
  end

  describe "access_token_ttl/0" do
    test "defaults to 7 days when unset" do
      System.delete_env("LAZYPOCK_AUTH_TOKEN_TTL")
      assert Token.access_token_ttl() == @default_ttl
    end

    test "reads a positive integer from the env" do
      System.put_env("LAZYPOCK_AUTH_TOKEN_TTL", "120")
      assert Token.access_token_ttl() == 120
    end

    test "falls back to the default on empty or invalid values" do
      System.put_env("LAZYPOCK_AUTH_TOKEN_TTL", "")
      assert Token.access_token_ttl() == @default_ttl

      System.put_env("LAZYPOCK_AUTH_TOKEN_TTL", "nope")
      assert Token.access_token_ttl() == @default_ttl

      System.put_env("LAZYPOCK_AUTH_TOKEN_TTL", "-5")
      assert Token.access_token_ttl() == @default_ttl

      System.put_env("LAZYPOCK_AUTH_TOKEN_TTL", "0")
      assert Token.access_token_ttl() == @default_ttl
    end
  end

  describe "LAZYPOCK_AUTH_TOKEN_TTL enforcement" do
    test "a token older than the configured TTL is rejected as expired" do
      System.put_env("LAZYPOCK_AUTH_TOKEN_TTL", "5")

      data = %{"id" => "u1", "email" => "old@test.com", "type" => "superuser"}

      token =
        Phoenix.Token.sign(Endpoint, "superuser", data, signed_at: System.os_time(:second) - 60)

      assert {:error, :expired} = Token.verify_token(token)
    end
  end

  describe "LAZYPOCK_AUTH_TOKEN_SECRET" do
    test "a dedicated secret signs and verifies tokens" do
      System.put_env("LAZYPOCK_AUTH_TOKEN_SECRET", "dedicated-secret-aaaaaaaaaaaaaaaa")
      su = superuser()

      {:ok, token} = Token.generate_access_token(su)
      assert {:ok, claims} = Token.verify_token(token)
      assert claims["id"] == su.id
    end

    test "changing the signing secret invalidates outstanding tokens" do
      su = superuser()

      System.put_env("LAZYPOCK_AUTH_TOKEN_SECRET", "secret-a-aaaaaaaaaaaaaaaaaaaaaa")
      {:ok, token} = Token.generate_access_token(su)

      System.put_env("LAZYPOCK_AUTH_TOKEN_SECRET", "secret-b-bbbbbbbbbbbbbbbbbbbbbb")
      assert {:error, _reason} = Token.verify_token(token)
    end
  end
end
