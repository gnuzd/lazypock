defmodule Lazypock.Auth.Token do
  @moduledoc """
  Token generation and verification for superuser authentication.

  Uses Phoenix's built-in token signing (HMAC) via the endpoint's
  secret_key_base — no extra dependencies needed.
  Tokens expire after 7 days (matching PocketBase default).
  """

  alias LazypockWeb.Endpoint

  @access_token_ttl 7 * 24 * 60 * 60  # 7 days in seconds

  @doc """
  Generates a signed access token for a superuser.
  The token contains the superuser's ID, email, and type.
  """
  @spec generate_access_token(map()) :: {:ok, String.t()}
  def generate_access_token(superuser) do
    data = %{
      "id" => superuser.id,
      "email" => superuser.email,
      "type" => "superuser"
    }

    token = Phoenix.Token.sign(Endpoint, "superuser", data)
    {:ok, token}
  end

  @doc """
  Verifies and decodes a signed access token.

  Returns `{:ok, claims_map}` or `{:error, reason}`.
  """
  @spec verify_token(String.t()) :: {:ok, map()} | {:error, term()}
  def verify_token(token) when is_binary(token) do
    case Phoenix.Token.verify(Endpoint, "superuser", token, max_age: @access_token_ttl) do
      {:ok, %{"type" => "superuser"} = data} ->
        {:ok, data}

      {:ok, _data} ->
        {:error, "Invalid token type"}

      {:error, reason} ->
        {:error, reason}
    end
  end
end
