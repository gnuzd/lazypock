defmodule Lazypock.Files.Storage do
  @moduledoc """
  Storage backend configuration: `local` (default) or `s3` (AWS S3 / Cloudflare
  R2 / MinIO).

  Resolved from `_settings.data["storage"]` with environment overrides — **env
  wins**, and `configured_from_env/0` tells the Studio which fields to lock.

      {
        "backend": "s3",
        "endpoint": "https://<account>.r2.cloudflarestorage.com",
        "region": "auto",
        "bucket": "my-bucket",
        "access_key_id": "...",
        "secret_access_key": "enc:...",
        "prefix": "lazypock/myapp/",
        "public_base_url": "https://cdn.example.com",
        "force_path_style": true,
        "presign_ttl": 900
      }

  `secret_access_key` is encrypted at rest (`Lazypock.Files.Storage.Secret`) and
  is never returned by the settings API. The resolved config is cached for two
  seconds, like the other settings readers, so a Studio change applies without a
  restart.
  """

  alias Lazypock.Files.Storage.Secret
  alias Lazypock.Settings

  @cache_key {__MODULE__, :cache}
  @ttl_ms 2_000
  @mask "••••••••"

  @defaults %{
    "backend" => "local",
    "region" => "auto",
    "prefix" => "",
    "force_path_style" => true,
    "presign_ttl" => 900
  }

  @env_vars %{
    "LAZYPOCK_S3_ENDPOINT" => "endpoint",
    "LAZYPOCK_S3_REGION" => "region",
    "LAZYPOCK_S3_BUCKET" => "bucket",
    "LAZYPOCK_S3_ACCESS_KEY" => "access_key_id",
    "LAZYPOCK_S3_SECRET" => "secret_access_key",
    "LAZYPOCK_S3_PUBLIC_URL" => "public_base_url",
    "LAZYPOCK_S3_PREFIX" => "prefix"
  }

  @doc "Resolved storage configuration (settings merged with env overrides)."
  def config do
    now = System.monotonic_time(:millisecond)

    case :persistent_term.get(@cache_key, nil) do
      {at, value} when now - at < @ttl_ms ->
        value

      _ ->
        value = load()
        :persistent_term.put(@cache_key, {now, value})
        value
    end
  end

  @doc "The configured backend name (`local` or `s3`)."
  def backend, do: config()["backend"] || "local"

  @doc "Whether the S3 backend is selected."
  def s3?, do: backend() == "s3"

  @doc "Config keys currently overridden by environment variables."
  def configured_from_env do
    for {env, key} <- @env_vars, System.get_env(env), do: key
  end

  @doc "Whether `key` is locked by an environment variable."
  def env_locked?(key), do: key in configured_from_env()

  @doc "Encrypt a secret for storage."
  def encrypt_secret(value), do: Secret.encrypt(value)

  @doc "The mask shown in place of the secret."
  def mask, do: @mask

  @doc "The config as the Studio may see it: secret replaced by a mask."
  def public_view do
    config()
    |> Map.put(
      "secret_access_key",
      if(blank?(config()["secret_access_key"]), do: "", else: @mask)
    )
    |> Map.put("secret_set", not blank?(config()["secret_access_key"]))
    |> Map.put("configured_from_env", configured_from_env())
  end

  @doc """
  Merge an incoming settings map over the persisted one, keeping the stored
  secret when the incoming value is blank or the display mask.
  """
  def merge(stored, incoming) when is_map(stored) and is_map(incoming) do
    incoming =
      case incoming["secret_access_key"] do
        nil -> Map.delete(incoming, "secret_access_key")
        "" -> Map.delete(incoming, "secret_access_key")
        @mask -> Map.delete(incoming, "secret_access_key")
        secret -> Map.put(incoming, "secret_access_key", Secret.encrypt(secret))
      end

    Map.merge(stored, incoming)
  end

  @doc "Validate a config map. Returns `:ok` or `{:error, message}`."
  def validate(%{"backend" => "s3"} = config) do
    cond do
      blank?(config["endpoint"]) -> {:error, "S3 endpoint is required"}
      blank?(config["bucket"]) -> {:error, "S3 bucket is required"}
      blank?(config["access_key_id"]) -> {:error, "S3 access key id is required"}
      blank?(config["secret_access_key"]) -> {:error, "S3 secret access key is required"}
      true -> :ok
    end
  end

  def validate(%{"backend" => "local"}), do: :ok

  def validate(%{"backend" => other}) when is_binary(other),
    do: {:error, "Unknown storage backend #{inspect(other)}"}

  def validate(_), do: :ok

  @doc false
  def clear_cache, do: :persistent_term.erase(@cache_key)

  # ── Internals ────────────────────────────────────────

  defp load do
    stored =
      case Settings.get("storage", %{}) do
        map when is_map(map) -> normalize(map)
        _ -> %{}
      end

    stored =
      case Secret.decrypt(stored["secret_access_key"]) do
        {:ok, secret} -> Map.put(stored, "secret_access_key", secret)
        {:error, :cannot_decrypt} -> Map.put(stored, "secret_error", true)
      end

    @defaults
    |> Map.merge(stored)
    |> Map.merge(env_overrides())
  rescue
    _ -> @defaults
  end

  defp env_overrides do
    for {env, key} <- @env_vars, value = System.get_env(env), value not in [nil, ""], into: %{} do
      {key, value}
    end
  end

  defp normalize(map) do
    map
    |> Map.put("backend", map["backend"] || "local")
    |> normalize_prefix()
    |> normalize_endpoint()
  end

  defp normalize_prefix(map) do
    case map["prefix"] do
      nil -> map
      "" -> map
      prefix -> Map.put(map, "prefix", String.trim_trailing(prefix, "/") <> "/")
    end
  end

  defp normalize_endpoint(map) do
    case map["endpoint"] do
      nil -> map
      endpoint -> Map.put(map, "endpoint", String.trim_trailing(endpoint, "/"))
    end
  end

  defp blank?(nil), do: true
  defp blank?(""), do: true
  defp blank?(value) when is_binary(value), do: String.trim(value) == ""
  defp blank?(_), do: false
end
