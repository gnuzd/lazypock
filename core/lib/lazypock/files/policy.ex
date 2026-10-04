defmodule Lazypock.Files.Policy do
  @moduledoc """
  Effective upload policy for a `file` / `multi_file` field.

  Precedence: **field option → global settings → built-in default**.

  The global settings live under `upload` in the `_settings` document (editable
  in the Studio, no restart needed). They are cached for a couple of seconds,
  like `Lazypock.CORS`, so a change takes effect quickly without a database
  round-trip on every request.

  ## Settings (`_settings.data["upload"]`)

      {
        "max_size": 10485760,          // bytes, or "10MB"
        "mime_types": ["image/*"],     // optional allowlist; nil = Validation decides
        "max_megapixels": 40,
        "max_dimension": 10000
      }

  Field options are PocketBase-compatible (`maxFileSize`, `mimeTypes`); the
  snake_case variants used by the settings document are also accepted.

  The environment overrides the size setting via `LAZYPOCK_UPLOAD_MAX_MB`.
  """

  alias Lazypock.Settings

  @cache_key {__MODULE__, :cache}
  @ttl_ms 2_000

  # Default retained at the historical 10 MB so existing installs do not start
  # rejecting uploads. Set `upload.max_size` (or `LAZYPOCK_UPLOAD_MAX_MB`) to
  # change it — PocketBase's own default is 5 MB.
  @defaults %{
    "max_size" => 10 * 1024 * 1024,
    "mime_types" => nil,
    "max_megapixels" => 40,
    "max_dimension" => 10_000
  }

  @doc "Resolve the effective policy for a file field's options map."
  @spec resolve(map()) :: map()
  def resolve(field_options \\ %{}) do
    @defaults
    |> Map.merge(settings())
    |> merge_field(field_options)
  end

  @doc "Maximum accepted file size in bytes."
  def max_size(policy), do: policy["max_size"] || @defaults["max_size"]

  @doc "Maximum megapixels (1 MP = 1_000_000 px) accepted."
  def max_megapixels(policy), do: policy["max_megapixels"] || @defaults["max_megapixels"]

  @doc "Maximum width or height accepted."
  def max_dimension(policy), do: policy["max_dimension"] || @defaults["max_dimension"]

  @doc """
  Whether `mime` is allowed by the policy.

  A `nil` allowlist means "no additional restriction" — `Lazypock.Files.Validation`
  still decides which types may be stored. Patterns may end in `/*`.
  """
  def mime_allowed?(_mime, %{"mime_types" => nil}), do: true
  def mime_allowed?(nil, _policy), do: false

  def mime_allowed?(mime, %{"mime_types" => allowed}) when is_list(allowed) do
    Enum.any?(allowed, fn pattern ->
      pattern = to_string(pattern)

      if String.ends_with?(pattern, "/*") do
        String.starts_with?(mime, String.trim_trailing(pattern, "*"))
      else
        mime == pattern
      end
    end)
  end

  def mime_allowed?(mime, policy), do: mime_allowed?(mime, Map.put(policy, "mime_types", nil))

  @doc """
  Checks an image's dimensions against the megapixel/dimension caps.

  Returns `:ok`, `{:error, reason}` or `:unknown` when ImageMagick is not
  installed. We only *advertise* the limit then — the size cap, the image
  limiter and ImageMagick's own memory limits still apply.
  """
  @spec check_image(String.t(), map()) ::
          :ok | :unknown | {:error, :image_too_large | :image_too_many_pixels | :overloaded}
  def check_image(path, policy) do
    case dimensions(path) do
      {:ok, w, h} ->
        cond do
          w > max_dimension(policy) or h > max_dimension(policy) -> {:error, :image_too_large}
          w * h > max_megapixels(policy) * 1_000_000 -> {:error, :image_too_many_pixels}
          true -> :ok
        end

      :unknown ->
        :unknown

      {:error, :overloaded} ->
        {:error, :overloaded}
    end
  end

  # ── Settings cache ───────────────────────────────────

  @doc false
  def settings do
    now = System.monotonic_time(:millisecond)

    case :persistent_term.get(@cache_key, nil) do
      {at, value} when now - at < @ttl_ms ->
        value

      _ ->
        value = load_settings()
        :persistent_term.put(@cache_key, {now, value})
        value
    end
  end

  @doc false
  def clear_cache, do: :persistent_term.erase(@cache_key)

  defp load_settings do
    configured =
      case Settings.get("upload", %{}) do
        map when is_map(map) -> map
        _ -> %{}
      end

    %{}
    |> put_size(configured["max_size"] || env_max_size() || app_config_max_size())
    |> put_int("max_megapixels", configured["max_megapixels"])
    |> put_int("max_dimension", configured["max_dimension"])
    |> put_list("mime_types", configured["mime_types"])
  rescue
    _ -> %{}
  end

  # Backwards compatibility with the pre-settings configuration.
  defp app_config_max_size do
    Application.get_env(:lazypock, Lazypock.Files.Store, [])
    |> Keyword.get(:max_file_size)
  end

  defp env_max_size do
    case Integer.parse(System.get_env("LAZYPOCK_UPLOAD_MAX_MB") || "") do
      {mb, _rest} when mb > 0 -> mb * 1_048_576
      _ -> nil
    end
  end

  defp put_size(map, nil), do: map

  defp put_size(map, bytes) when is_integer(bytes) and bytes > 0,
    do: Map.put(map, "max_size", bytes)

  defp put_size(map, value) when is_binary(value) do
    case parse_size(value) do
      nil -> map
      bytes -> Map.put(map, "max_size", bytes)
    end
  end

  defp put_size(map, _other), do: map

  defp put_int(map, _key, nil), do: map

  defp put_int(map, key, value) when is_integer(value) and value > 0, do: Map.put(map, key, value)

  defp put_int(map, key, value) when is_binary(value) do
    case Integer.parse(value) do
      {n, _} when n > 0 -> Map.put(map, key, n)
      _ -> map
    end
  end

  defp put_int(map, _key, _other), do: map

  defp put_list(map, _key, nil), do: map

  defp put_list(map, key, list) when is_list(list) and list != [],
    do: Map.put(map, key, Enum.map(list, &to_string/1))

  defp put_list(map, _key, _other), do: map

  @doc false
  def parse_size(value) when is_binary(value) do
    value = value |> String.trim() |> String.downcase()

    case Regex.run(~r/^(\d+(?:\.\d+)?)\s*(b|kb|mb|gb|)$/, value) do
      [_, amount, unit] ->
        {n, _} = Float.parse(amount)

        multiplier =
          case unit do
            "gb" -> 1_073_741_824
            "mb" -> 1_048_576
            "kb" -> 1024
            _ -> 1
          end

        round(n * multiplier)

      _ ->
        nil
    end
  end

  # ── Field options ────────────────────────────────────

  defp merge_field(policy, opts) when is_map(opts) do
    policy
    |> put_size_option(opts["maxFileSize"] || opts["max_size"])
    |> put_list("mime_types", opts["mimeTypes"] || opts["mime_types"])
  end

  defp merge_field(policy, _opts), do: policy

  defp put_size_option(policy, nil), do: policy

  defp put_size_option(policy, value) do
    cond do
      is_integer(value) and value > 0 ->
        Map.put(policy, "max_size", value)

      is_binary(value) ->
        case parse_size(value) do
          nil -> policy
          bytes -> Map.put(policy, "max_size", bytes)
        end

      true ->
        policy
    end
  end

  # ── Image dimensions (ImageMagick header read) ───────

  # The engine owns the ImageMagick invocation details (including the IM6/IM7
  # difference); this only adds the limiter and maps any failure to `:unknown`,
  # so the dimension caps stay best-effort when the image cannot be read.
  defp dimensions(path) do
    case Lazypock.Files.Limiter.run(fn -> Lazypock.Images.engine().dimensions(path) end) do
      {:ok, {:ok, w, h}} -> {:ok, w, h}
      {:ok, {:error, _reason}} -> :unknown
      {:error, :overloaded} -> {:error, :overloaded}
      _ -> :unknown
    end
  rescue
    _ -> :unknown
  end
end
