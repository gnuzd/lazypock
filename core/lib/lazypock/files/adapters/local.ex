defmodule Lazypock.Files.Adapters.Local do
  @moduledoc """
  Local filesystem adapter.

  Stores files in `priv/uploads/` organized by date:
    priv/uploads/YYYY/MM/DD/{uuid}.{ext}
  """

  @behaviour Lazypock.Files.Adapter

  @doc """
  Stores a file on the local filesystem.
  """
  @impl true
  def store(binary, filename, _opts) do
    ext = Path.extname(filename)
    uuid = Ecto.UUID.generate()
    date_path = date_based_path()
    storage_path = Path.join([base_path(), date_path, "#{uuid}#{ext}"])
    dir = Path.dirname(storage_path)
    File.mkdir_p!(dir)

    case File.write(storage_path, binary) do
      :ok ->
        {:ok,
         %{
           path: Path.join(date_path, "#{uuid}#{ext}"),
           size: byte_size(binary),
           mime_type: mime_type(ext)
         }}

      {:error, reason} ->
        {:error, reason}
    end
  end

  @impl true
  def url(file_record) do
    "/api/files/#{file_record.id}"
  end

  @impl true
  def get(file_record) do
    full_path = Path.join(base_path(), file_record.storage_path)

    case File.read(full_path) do
      {:ok, binary} -> {:ok, binary}
      {:error, reason} -> {:error, reason}
    end
  end

  @impl true
  def delete(file_record) do
    full_path = Path.join(base_path(), file_record.storage_path)
    File.rm(full_path)
    # Try to clean up empty parent dirs (ignore errors)
    clean_empty_dirs(Path.dirname(full_path))
    :ok
  end

  defp base_path do
    Application.get_env(:lazypock, :file_storage)[:path] ||
      System.get_env("LAZYPOCK_FILE_STORAGE_PATH") ||
      Path.join(Application.app_dir(:lazypock, "priv"), "uploads")
  end

  defp date_based_path do
    now = DateTime.utc_now()
    "#{now.year}/#{pad(now.month)}/#{pad(now.day)}"
  end

  defp pad(n) when n < 10, do: "0#{n}"
  defp pad(n), do: to_string(n)

  defp mime_type(".jpg"), do: "image/jpeg"
  defp mime_type(".jpeg"), do: "image/jpeg"
  defp mime_type(".png"), do: "image/png"
  defp mime_type(".gif"), do: "image/gif"
  defp mime_type(".webp"), do: "image/webp"
  defp mime_type(".svg"), do: "image/svg+xml"
  defp mime_type(".pdf"), do: "application/pdf"
  defp mime_type(".mp4"), do: "video/mp4"
  defp mime_type(".mp3"), do: "audio/mpeg"
  defp mime_type(".json"), do: "application/json"
  defp mime_type(".csv"), do: "text/csv"
  defp mime_type(".txt"), do: "text/plain"
  defp mime_type(".zip"), do: "application/zip"
  defp mime_type(_), do: "application/octet-stream"

  defp clean_empty_dirs(dir) do
    case File.ls(dir) do
      {:ok, []} ->
        File.rmdir(dir)
        clean_empty_dirs(Path.dirname(dir))

      _ ->
        :ok
    end
  rescue
    _ -> :ok
  end
end
