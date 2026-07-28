defmodule Lazypock.Auth.Token do
  @moduledoc """
  Token generation and verification for superuser and auth collection user authentication.

  Uses Phoenix's built-in token signing (HMAC) via the endpoint's
  secret_key_base — no extra dependencies needed.
  Tokens expire after 7 days (matching PocketBase default).
  """

  alias LazypockWeb.Endpoint

  # 7 days in seconds
  @access_token_ttl 7 * 24 * 60 * 60

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

    token = Phoenix.Token.sign(Endpoint, "superuser", data)
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

    token = Phoenix.Token.sign(Endpoint, "auth_user", data)
    {:ok, token}
  end

  @doc """
  Verifies and decodes a signed superuser access token.
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

  @doc """
  Verifies and decodes a signed auth collection user access token.
  """
  @spec verify_user_token(String.t()) :: {:ok, map()} | {:error, term()}
  def verify_user_token(token) when is_binary(token) do
    case Phoenix.Token.verify(Endpoint, "auth_user", token, max_age: @access_token_ttl) do
      {:ok, %{"type" => "user"} = data} ->
        {:ok, data}

      {:ok, _data} ->
        {:error, "Invalid token type"}

      {:error, reason} ->
        {:error, reason}
    end
  end
end
