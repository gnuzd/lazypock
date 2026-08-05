defmodule Lazypock.Files.Adapter do
  @moduledoc """
  Behaviour for file storage backends.

  Implementations:
    - Lazypock.Files.Adapters.Local   (default, files on disk)
    - Lazypock.Files.Adapters.S3      (AWS S3 / Cloudflare R2 compatible)

  Default is always local, zero config. S3 can be configured later
  via Admin UI (stored in _settings table in Phase 8).
  """

  @type file_meta :: %{
          required(:path) => String.t(),
          required(:size) => non_neg_integer(),
          required(:mime_type) => String.t(),
          optional(:width) => non_neg_integer(),
          optional(:height) => non_neg_integer()
        }

  @callback store(binary(), String.t(), keyword()) :: {:ok, file_meta()} | {:error, term()}
  @callback url(map()) :: String.t()
  @callback get(map()) :: {:ok, binary()} | {:error, term()}
  @callback delete(map()) :: :ok | {:error, term()}

  # Optional: generate thumbnails from an image binary. Returns a list of thumb
  # meta maps (each with :size, :path, :width, :height, :mime_type) or [] if
  # unsupported. Only implemented by adapters that can resize images.
  @callback thumbs(binary(), String.t(), [String.t()]) :: {:ok, [map()]} | {:error, term()}

  @optional_callbacks thumbs: 3

  @doc """
  Returns the adapter for a given backend name from _files.storage_backend.
  Always defaults to local adapter.
  """
  def for_backend("s3"), do: Lazypock.Files.Adapters.S3
  def for_backend(_), do: Lazypock.Files.Adapters.Local
end
