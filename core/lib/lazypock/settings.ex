defmodule Lazypock.Settings do
  @moduledoc """
  Read/write access to the application settings stored in the `_settings` table.

  Settings are stored as a single JSON document keyed by the `data` column.
  This module is shared by the admin Settings UI and the auth pipeline
  (so API-key authentication can live in `Lazypock.Auth.Plug`).

  ## API keys

  API keys are stored as a **list** of metadata maps. Only the SHA-256 digest
  of each raw key is persisted — the raw value is shown exactly once at
  generation and can never be recovered later. Each key carries:

    * `id` — stable identifier (used to revoke)
    * `hash` — SHA-256 digest of the raw key
    * `created_at` — ISO-8601 timestamp
    * `expires_at` — ISO-8601 timestamp, or `nil` for never-expiring
    * `revoked` — boolean; revoked keys no longer authenticate
  """

  alias Lazypock.Repo

  # Settings key holding the list of API-key metadata.
  @api_keys_key "api_keys"
  # Legacy key used before keys were a list (a single hashed string).
  @legacy_api_key_key "api_key"
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

  # ── API keys ──────────────────────────────────────────

  @doc "Return the list of API-key metadata maps (empty list when none)."
  @spec list_api_keys() :: [map()]
  def list_api_keys do
    persisted_keys()
    |> Enum.map(&public_key_meta/1)
  end

  # Return the raw (persisted) key metadata list used internally.
  defp persisted_keys do
    keys = get(@api_keys_key, []) |> normalize_keys()

    # Lazily migrate a legacy single key into the list an append to it
    # so it keeps working and appears in the dashboard.
    if keys == [] and legacy_api_key_hash() do
      migrated = [
        %{
          "id" => Ecto.UUID.generate(),
          "hash" => legacy_api_key_hash(),
          "created_at" => DateTime.utc_now() |> DateTime.to_iso8601(),
          "expires_at" => nil,
          "revoked" => false
        }
      ]

      put(Map.put(get(), @api_keys_key, migrated))
      migrated
    else
      keys
    end
  end

  @doc """
  Create a new API key. `expires_in` is an ISO-compatible offset in integer
  days (nil/0 means never expires).

  Returns `{raw_key, meta}` where `raw_key` is shown exactly once and `meta`
  is the persistable metadata (which is what the UI should store/display).
  """
  @spec create_api_key(integer() | nil) :: {String.t(), map()}
  def create_api_key(expires_in \\ nil) do
    key = new_api_key()
    now = DateTime.utc_now()

    meta = %{
      "id" => Ecto.UUID.generate(),
      "hash" => hash_key(key),
      "created_at" => DateTime.to_iso8601(now),
      "expires_at" => expires_at(expires_in, now),
      "revoked" => false
    }

    keys = persisted_keys() ++ [meta]
    put(Map.merge(get(), %{@api_keys_key => keys}))
    {key, public_key_meta(meta)}
  end

  @doc """
  Verify a presented API key: hash must match an active (non-revoked,
  non-expired) stored key. Constant-time. Returns false otherwise.
  """
  @spec verify_api_key(String.t() | nil) :: boolean()
  def verify_api_key(nil), do: false

  def verify_api_key(key) when is_binary(key) do
    now = DateTime.utc_now()
    presented = hash_key(key)

    # Legacy single-key check for back-compat.
    Enum.any?(persisted_keys(), fn meta ->
      not meta["revoked"] and meta["hash"] == presented and not expired?(meta, now)
    end) ||
      case legacy_api_key_hash() do
        hash when is_binary(hash) -> secure_compare(presented, hash)
        _ -> false
      end
  end

  @doc "Return true when at least one active (non-revoked) key exists."
  @spec has_api_key?() :: boolean()
  def has_api_key? do
    now = DateTime.utc_now()

    Enum.any?(persisted_keys(), fn meta ->
      not meta["revoked"] and not expired?(meta, now)
    end) || !!legacy_api_key_hash()
  end

  @doc "Revoke an API key by id (keeps it in the list, marked revoked)."
  @spec revoke_api_key(String.t()) :: :ok | :error
  def revoke_api_key(id) when is_binary(id) do
    keys = persisted_keys()

    case Enum.find(keys, &(&1["id"] == id)) do
      nil ->
        :error

      _found ->
        updated =
          Enum.map(keys, fn
            %{"id" => ^id} = meta -> Map.put(meta, "revoked", true)
            meta -> meta
          end)

        put(Map.merge(get(), %{@api_keys_key => updated}))
        :ok
    end
  end

  @doc "Permanently remove an API key by id."
  @spec delete_api_key!(String.t()) :: :ok | :error
  def delete_api_key!(id) when is_binary(id) do
    keys = persisted_keys()

    if Enum.any?(keys, &(&1["id"] == id)) do
      put(Map.merge(get(), %{@api_keys_key => Enum.reject(keys, &(&1["id"] == id))}))
      :ok
    else
      :error
    end
  end

  @doc "SHA-256 digest of a key, hex-encoded."
  @spec hash_key(String.t()) :: String.t()
  def hash_key(key) when is_binary(key) do
    :crypto.hash(:sha256, key) |> Base.encode16(case: :lower)
  end

  @doc """
  Generate a new random API key: `lazypock_` + 32 URL-safe base64 chars.
  """
  @spec new_api_key() :: String.t()
  def new_api_key do
    @api_key_prefix <>
      (:crypto.strong_rand_bytes(24) |> Base.url_encode64(padding: false))
  end

  @doc "Constant-time string equality."
  @spec secure_compare(String.t(), String.t()) :: boolean()
  def secure_compare(a, b) do
    Plug.Crypto.secure_compare(a, b)
  end

  # ── Private helpers ───────────────────────────────────

  # Normalize a value that may be a list, or a single legacy key string,
  # into a list of metadata maps.
  defp normalize_keys(nil), do: []
  defp normalize_keys(keys) when is_list(keys), do: keys

  defp legacy_api_key_hash do
    case get(@legacy_api_key_key) do
      hash when is_binary(hash) and hash != "" -> hash
      _ -> nil
    end
  end

  defp expired?(%{"expires_at" => nil}, _now), do: false

  defp expired?(%{"expires_at" => exp}, now) do
    case DateTime.from_iso8601(exp) do
      {:ok, dt, _} -> DateTime.compare(now, dt) == :gt
      _ -> false
    end
  end

  # Build the ISO expires_at from a whole-day offset.
  defp expires_at(nil, _now), do: nil

  defp expires_at(days, now) when is_integer(days) and days > 0 do
    now
    |> DateTime.add(days * 86_400, :second)
    |> DateTime.to_iso8601()
  end

  defp expires_at(_days, _now), do: nil

  # Public metadata returned to callers: no internal fields leaked.
  defp public_key_meta(meta) do
    %{
      "id" => meta["id"],
      "created_at" => meta["created_at"],
      "expires_at" => meta["expires_at"],
      "revoked" => meta["revoked"]
    }
  end
end
