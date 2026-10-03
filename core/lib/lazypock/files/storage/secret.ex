defmodule Lazypock.Files.Storage.Secret do
  @moduledoc """
  Encrypt/decrypt the S3 secret access key at rest.

  AES-256-GCM with a key derived (HMAC-SHA256, HKDF-style extract) from the
  application's `SECRET_KEY_BASE`. Ciphertext is stored as `"enc:" <> base64(iv
  || tag || ciphertext)` inside `_settings.data.storage`.

  **Rotating `SECRET_KEY_BASE` makes a stored secret undecryptable.** When that
  happens `decrypt/1` returns `{:error, :cannot_decrypt}` and the Studio asks for
  the secret again rather than silently using a corrupted value.
  """

  @aad "lazypock/storage/secret"
  @prefix "enc:"

  @doc "Encrypt a plaintext secret. Idempotent for already-encrypted values."
  def encrypt(@prefix <> _ = already), do: already
  def encrypt(nil), do: nil
  def encrypt(""), do: ""

  def encrypt(plaintext) when is_binary(plaintext) do
    iv = :crypto.strong_rand_bytes(12)

    {ciphertext, tag} =
      :crypto.crypto_one_time_aead(:aes_256_gcm, key(), iv, plaintext, @aad, 16, true)

    @prefix <> Base.encode64(iv <> tag <> ciphertext)
  end

  @doc "Decrypt a stored secret. Plaintext values pass through unchanged."
  @spec decrypt(term()) :: {:ok, String.t() | nil} | {:error, :cannot_decrypt}
  def decrypt(nil), do: {:ok, nil}
  def decrypt(""), do: {:ok, ""}

  def decrypt(@prefix <> payload) do
    with {:ok, decoded} <- Base.decode64(payload),
         <<iv::binary-size(12), tag::binary-size(16), ciphertext::binary>> <- decoded do
      case :crypto.crypto_one_time_aead(:aes_256_gcm, key(), iv, ciphertext, @aad, tag, false) do
        :error -> {:error, :cannot_decrypt}
        plaintext -> {:ok, plaintext}
      end
    else
      _ -> {:error, :cannot_decrypt}
    end
  end

  def decrypt(plaintext) when is_binary(plaintext), do: {:ok, plaintext}

  @doc "Whether a value is encrypted at rest."
  def encrypted?(@prefix <> _), do: true
  def encrypted?(_), do: false

  defp key do
    :crypto.mac(:hmac, :sha256, secret_key_base(), "lazypock/storage/secret")
  end

  defp secret_key_base do
    System.get_env("SECRET_KEY_BASE") ||
      Application.get_env(:lazypock, LazypockWeb.Endpoint, [])[:secret_key_base] ||
      raise "SECRET_KEY_BASE is not configured; cannot encrypt the storage secret"
  end
end
