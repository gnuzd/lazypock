defmodule Lazypock.Auth.Token do
  @moduledoc """
  Token generation and verification for superuser and auth collection user
  authentication.

  Tokens are **HMAC-signed `Phoenix.Token` values, not JWTs** — there is no
  extra dependency. The signing key comes from the endpoint's
  `secret_key_base` by default, or from a dedicated
  `LAZYPOCK_AUTH_TOKEN_SECRET` when that env var is set (recommended in
  production so rotating the cookie signing key and the token key are
  independent operations).

  Tokens expire after 7 days by default. Set `LAZYPOCK_AUTH_TOKEN_TTL` to a
  positive number of seconds to change the lifetime.

  ## Revocation

  Tokens are stateless and cannot be revoked before they expire. There is no
  `_auth_tokens` table (it is an aspirational entry in PLAN.md). To invalidate
  every outstanding token, rotate `SECRET_KEY_BASE` /
  `LAZYPOCK_AUTH_TOKEN_SECRET`, or shorten `LAZYPOCK_AUTH_TOKEN_TTL`.
  """

  alias LazypockWeb.Endpoint

  # 7 days in seconds (PocketBase default). Overridable at runtime.
  @default_access_token_ttl 7 * 24 * 60 * 60

  @superuser_salt "superuser"
  @user_salt "auth_user"

  @doc """
  Configured access-token lifetime in seconds.

  Reads `LAZYPOCK_AUTH_TOKEN_TTL` at call time; falls back to 7 days when it
  is unset, empty, or not a positive integer.
  """
  @spec access_token_ttl() :: pos_integer()
  def access_token_ttl do
    case System.get_env("LAZYPOCK_AUTH_TOKEN_TTL") do
      nil ->
        @default_access_token_ttl

      "" ->
        @default_access_token_ttl

      value ->
        case Integer.parse(value) do
          {seconds, _rest} when seconds > 0 -> seconds
          _ -> @default_access_token_ttl
        end
    end
  end

  @doc """
  Generates a signed access token for a superuser.
  """
  @spec generate_access_token(map()) :: {:ok, String.t()}
  def generate_access_token(superuser) do
    data = %{
      "id" => superuser.id,
      "email" => superuser.email,
      "type" => "superuser"
    }

    token = Phoenix.Token.sign(signer(), @superuser_salt, data)
    {:ok, token}
  end

  @doc """
  Generates a signed access token for an auth collection user (record).
  The token includes the user's ID, email, and collection name so the
  plug can resolve `@request.auth.*` tokens during rule enforcement.
  """
  @spec generate_user_token(map(), String.t()) :: {:ok, String.t()} | {:error, term()}
  def generate_user_token(record, collection_name) do
    data = %{
      "id" => record["id"],
      "email" => record["email"] || "",
      "collectionName" => collection_name,
      "type" => "user"
    }

    token = Phoenix.Token.sign(signer(), @user_salt, data)
    {:ok, token}
  end

  @doc """
  Verifies and decodes a signed superuser access token.
  """
  @spec verify_token(String.t()) :: {:ok, map()} | {:error, term()}
  def verify_token(token) when is_binary(token) do
    case Phoenix.Token.verify(signer(), @superuser_salt, token, max_age: access_token_ttl()) do
      {:ok, %{"type" => "superuser"} = data} ->
        {:ok, data}

      {:ok, _data} ->
        {:error, "Invalid token type"}

      {:error, reason} ->
        {:error, reason}
    end
  end

  @doc """
  Verifies and decodes a signed auth collection user access token.
  """
  @spec verify_user_token(String.t()) :: {:ok, map()} | {:error, term()}
  def verify_user_token(token) when is_binary(token) do
    case Phoenix.Token.verify(signer(), @user_salt, token, max_age: access_token_ttl()) do
      {:ok, %{"type" => "user"} = data} ->
        {:ok, data}

      {:ok, _data} ->
        {:error, "Invalid token type"}

      {:error, reason} ->
        {:error, reason}
    end
  end

  # Signing context: a dedicated secret when configured, otherwise the
  # endpoint module (whose `secret_key_base` is used). Passed to
  # `Phoenix.Token` as either a binary secret or a module.
  defp signer do
    case System.get_env("LAZYPOCK_AUTH_TOKEN_SECRET") do
      secret when is_binary(secret) and secret != "" -> secret
      _ -> Endpoint
    end
  end
end
