defmodule Lazypock.Files.Adapter do
  @moduledoc """
  Behaviour for file storage backends.

  Implementations:
    - Lazypock.Files.Adapters.Local   (default, files on disk)
    - Lazypock.Files.Adapters.S3      (AWS S3 / Cloudflare R2 compatible)
  """

  @type file_meta :: %{
    required(:path) => String.t(),
    required(:size) => non_neg_integer(),
    required(:mime_type) => String.t(),
    optional(:width) => non_neg_integer(),
    optional(:height) => non_neg_integer()
  }

  @doc """
  Stores a file and returns metadata.
  """
  @callback store(binary(), String.t(), keyword()) :: {:ok, file_meta()} | {:error, term()}

  @doc """
  Returns the full local path or signed URL for a stored file.
  """
  @callback url(map()) :: String.t()

  @doc """
  Returns the file binary.
  """
  @callback get(map()) :: {:ok, binary()} | {:error, term()}

  @doc """
  Deletes a stored file.
  """
  @callback delete(map()) :: :ok | {:error, term()}

  @doc """
  Returns the configured adapter module based on app env.
  """
  def get_adapter do
    case Application.get_env(:lazypock, :file_storage)[:adapter] do
      :s3 -> Lazypock.Files.Adapters.S3
      _ -> Lazypock.Files.Adapters.Local
    end
  end
end
