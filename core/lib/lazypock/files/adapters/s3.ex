defmodule Lazypock.Files.Adapters.S3 do
  @moduledoc """
  S3-compatible storage adapter (AWS S3, Cloudflare R2, etc.).

  Configuration via env vars:
    LAZYPOCK_S3_BUCKET
    LAZYPOCK_S3_REGION
    LAZYPOCK_S3_ACCESS_KEY_ID
    LAZYPOCK_S3_SECRET_ACCESS_KEY
    LAZYPOCK_S3_ENDPOINT (optional, for Cloudflare R2 or MinIO)
    LAZYPOCK_S3_PUBLIC_URL (optional, for CDN)
  """

  @behaviour Lazypock.Files.Adapter

  @impl true
  def store(_binary, _filename, _opts) do
    {:error,
     "S3 adapter not yet implemented — use local adapter or configure file_storage adapter"}
  end

  @impl true
  def url(_file_record) do
    ""
  end

  @impl true
  def get(_file_record) do
    {:error, "S3 adapter not yet implemented"}
  end

  # Backup support. A remote object has no local file, so an export falls back to
  # `get/1` (which buffers the object) instead of copying on disk.
  @impl true
  def local_path(_file_record), do: :error

  # Backup support: write at an explicit storage key. Implement this alongside
  # `store/3` — restoring a backup must preserve `_files.storage_path`, and
  # `store/3` generates a fresh path.
  @impl true
  def put_at(_storage_path, _source, _opts) do
    {:error, "S3 adapter not yet implemented — cannot restore files to S3"}
  end

  @impl true
  def delete(_file_record) do
    {:error, "S3 adapter not yet implemented"}
  end

  @impl true
  def thumbs(_binary, _filename, _sizes) do
    {:ok, []}
  end

  @impl true
  def scale(_file_record, _size) do
    {:error, "S3 adapter not yet implemented — on-demand scaling unavailable"}
  end

  def thumb_get(_file_record, _thumb) do
    {:error, "S3 adapter not yet implemented"}
  end
end
