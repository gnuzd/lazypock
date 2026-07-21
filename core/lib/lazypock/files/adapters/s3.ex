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
    {:error, "S3 adapter not yet implemented — use local adapter or configure file_storage adapter"}
  end

  @impl true
  def url(_file_record) do
    ""
  end

  @impl true
  def get(_file_record) do
    {:error, "S3 adapter not yet implemented"}
  end

  @impl true
  def delete(_file_record) do
    {:error, "S3 adapter not yet implemented"}
  end
end
