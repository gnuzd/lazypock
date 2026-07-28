defmodule LazypockWeb.FileController do
  use LazypockWeb, :controller

  alias Lazypock.Files.Store
  alias Lazypock.Collections.Registry

  @default_max_file_size 10 * 1024 * 1024

  @doc """
  POST /api/files
  Upload a file (multipart/form-data).

  Supports an optional `collection_name` field in the multipart body.
  When provided, the max file size is resolved from:
    1. The file field's `options.maxFileSize` in the collection schema
    2. App config: `config :lazypock, Lazypock.Files.Store, max_file_size: N`
    3. Default: 10 MB
  """
  def upload(conn, %{"file" => upload}) do
    max_size = resolve_max_file_size(conn.params)

    if upload.size > max_size do
      conn
      |> put_status(413)
      |> json(%{
        "code" => 413,
        "message" => "File too large. Maximum size is #{format_bytes(max_size)}.",
        "data" => %{}
      })
    else
      do_upload(conn, upload)
    end
  end

  defp do_upload(conn, upload) do
    cond do
      upload.content_type not in ~w(image/jpeg image/png image/gif image/webp
                                    application/pdf text/csv text/plain
                                    application/json application/zip
                                    video/mp4 audio/mpeg) ->
        conn
        |> put_status(400)
        |> json(%{"code" => 400, "message" => "File type not allowed", "data" => %{}})

      true ->
        case Store.store(upload) do
          {:ok, file_record} ->
            conn
            |> put_status(201)
            |> json(format_file(file_record))

          {:error, reason} ->
            conn
            |> put_status(400)
            |> json(%{"code" => 400, "message" => inspect(reason), "data" => %{}})
        end
    end
  end

  @doc """
  GET /api/files/:id
  Serve a file.
  """
  def show(conn, %{"id" => id}) do
    case Store.get(id) do
      {:ok, file_record} ->
        case Store.read(file_record) do
          {:ok, binary} ->
            conn
            |> put_resp_header("content-type", file_record["mime_type"])
            |> put_resp_header(
              "content-disposition",
              ~s(inline; filename="#{file_record["filename"]}")
            )
            |> send_resp(200, binary)

          {:error, reason} ->
            conn
            |> put_status(500)
            |> json(%{"code" => 500, "message" => inspect(reason), "data" => %{}})
        end

      {:error, :not_found} ->
        conn
        |> put_status(404)
        |> json(%{"code" => 404, "message" => "File not found", "data" => %{}})
    end
  end

  @doc """
  DELETE /api/files/:id
  Delete a file.
  """
  def delete(conn, %{"id" => id}) do
    case Store.delete(id) do
      :ok ->
        conn |> put_status(204) |> json(nil)

      {:error, reason} ->
        conn
        |> put_status(400)
        |> json(%{"code" => 400, "message" => inspect(reason), "data" => %{}})
    end
  end

  # ── Size limit helpers ────────────────────────────────

  defp resolve_max_file_size(params) do
    case params["collection_name"] do
      nil ->
        global_config()

      collection_name when is_binary(collection_name) and collection_name != "" ->
        collection_max_size(collection_name) || global_config()
    end
  end

  defp collection_max_size(collection_name) do
    with {:ok, collection} <- Registry.get(collection_name),
         fields <- collection.fields || [],
         file_field <- Enum.find(fields, fn f -> f.type == "file" end) do
      if file_field && is_integer(file_field.options["maxFileSize"]) do
        file_field.options["maxFileSize"]
      else
        nil
      end
    else
      _ -> nil
    end
  end

  defp global_config do
    Application.get_env(:lazypock, Lazypock.Files.Store, [])
    |> Keyword.get(:max_file_size, @default_max_file_size)
  end

  defp format_bytes(bytes) when is_integer(bytes) do
    cond do
      bytes >= 1_073_741_824 ->
        format_size(bytes / 1_073_741_824, "GB")

      bytes >= 1_048_576 ->
        format_size(bytes / 1_048_576, "MB")

      bytes >= 1024 ->
        format_size(bytes / 1024, "KB")

      true ->
        "#{bytes} bytes"
    end
  end

  defp format_size(value, unit) do
    rounded = Float.round(value, 1)

    if rounded == trunc(rounded) do
      "#{trunc(rounded)} #{unit}"
    else
      "#{rounded} #{unit}"
    end
  end

  defp format_file(file_record) do
    %{
      "id" => file_record["id"],
      "filename" => file_record["filename"],
      "mimeType" => file_record["mime_type"],
      "size" => file_record["size"],
      "url" => Store.url(file_record)
    }
  end
end