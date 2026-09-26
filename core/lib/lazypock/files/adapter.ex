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

  # Optional: on-demand scale an image to an arbitrary size. Reads the original
  # file binary, generates a resized version (cached), and returns the resized
  # binary + mime type. Used by GET /api/files/:id/scale/:size.
  @callback scale(map(), String.t()) ::
              {:ok, binary(), String.t()} | {:error, term()}

  # Optional: absolute on-disk path of a stored object, when the backend has one.
  # Lets a backup copy bytes with `File.cp/2` instead of loading them into memory.
  # Return `:error` when there is no local file (e.g. a remote object store); the
  # backup then falls back to `get/1`.
  @callback local_path(map()) :: {:ok, String.t()} | :error

  # Write bytes at an EXPLICIT storage path. `store/3` generates its own path,
  # which is wrong when restoring a backup (the path is referenced by
  # `_files.storage_path` and must be preserved). `source` is either a binary or
  # `{:file, path}` so a local backend can copy without buffering.
  @callback put_at(String.t(), binary() | {:file, String.t()}, keyword()) ::
              :ok | {:error, term()}

  @optional_callbacks thumbs: 3, scale: 2

  @doc """
  Returns the adapter for a given backend name from _files.storage_backend.
  Always defaults to local adapter.
  """
  def for_backend("s3"), do: Lazypock.Files.Adapters.S3
  def for_backend(_), do: Lazypock.Files.Adapters.Local
end
