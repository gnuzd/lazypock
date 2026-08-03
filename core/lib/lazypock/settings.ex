defmodule Lazypock.Settings do
  @moduledoc """
  Read/write access to the application settings stored in the `_settings` table.

  Settings are stored as a single JSON document keyed by the `data` column.
  This module is shared by the admin Settings UI and the auth pipeline
  (so API-key authentication can live in `Lazypock.Auth.Plug`).
  """

  alias Lazypock.Repo

  # Settings key holding the API key hash (never the raw key).
  # Only the SHA-256 digest of the key is persisted, so a leaked settings
  # export cannot be replayed as a valid credential.
  @api_key_key "api_key"
  @api_key_prefix "lazypock_"

  @doc "Return the full settings map (empty map when nothing is stored)."
  def get do
    case Ecto.Adapters.SQL.query(
           Repo,
           "SELECT data FROM _settings LIMIT 1",
           []
         ) do
      {:ok, %{rows: [[data]]}} when is_map(data) ->
        data

      {:ok, %{rows: [[data]]}} when is_binary(data) ->
        case Jason.decode(data) do
          {:ok, decoded} when is_map(decoded) -> decoded
          _ -> %{}
        end

      _ ->
        %{}
    end
  end

  @doc "Overwrite the settings with the given map."
  def put(data) when is_map(data) do
    case Ecto.Adapters.SQL.query(
           Repo,
           "UPDATE _settings SET data = $1, updated_at = now() WHERE id = (SELECT id FROM _settings LIMIT 1)",
           [data]
         ) do
      {:ok, %{num_rows: 0}} ->
        Ecto.Adapters.SQL.query!(
          Repo,
          "INSERT INTO _settings (data) VALUES ($1)",
          [data]
        )

      _ ->
        :ok
    end
  end

  @doc "Fetch a single setting value by key, or `default`."
  def get(key, default \\ nil) do
    Map.get(get(), key, default)
  end

  @doc """
  Verify a presented API key against the stored SHA-256 hash.
  Constant-time comparison. Returns false when no key is configured.
  """
  @spec verify_api_key(String.t() | nil) :: boolean()
  def verify_api_key(nil), do: false

  def verify_api_key(key) when is_binary(key) do
    case get(@api_key_key) do
      stored_hash when is_binary(stored_hash) and stored_hash != "" ->
        secure_compare(hash_key(key), stored_hash)

      _ ->
        false
    end
  end

  @doc "Return true when an API key has been configured (any key exists)."
  @spec has_api_key?() :: boolean()
  def has_api_key? do
    case get(@api_key_key) do
      hash when is_binary(hash) and hash != "" -> true
      _ -> false
    end
  end

  @doc "Store a new API key (persists only its SHA-256 hash)."
  @spec set_api_key(String.t()) :: :ok
  def set_api_key(key) do
    data = get()
    put(Map.put(data, @api_key_key, hash_key(key)))
    :ok
  end

  @doc "SHA-256 digest of a key, hex-encoded."
  @spec hash_key(String.t()) :: String.t()
  def hash_key(key) when is_binary(key) do
    :crypto.hash(:sha256, key) |> Base.encode16(case: :lower)
  end

  @doc "Remove the API key."
  @spec clear_api_key() :: :ok
  def clear_api_key do
    data = get()
    put(Map.delete(data, @api_key_key))
    :ok
  end

  @doc """
  Generate a new random API key: `lazypock_` + 32 URL-safe base64 chars.
  The raw key is returned to the caller exactly once (shown in the UI);
  only its hash is persisted via `set_api_key/1`.
  """
  @spec new_api_key() :: String.t()
  def new_api_key do
    @api_key_prefix <>
      (:crypto.strong_rand_bytes(24) |> Base.url_encode64(padding: false))
  end

  @doc "Constant-time string equality for API key comparison."
  @spec secure_compare(String.t(), String.t()) :: boolean()
  def secure_compare(a, b) do
    Plug.Crypto.secure_compare(a, b)
  end
end
