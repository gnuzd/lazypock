defmodule LazypockWeb.FileController do
  use LazypockWeb, :controller

  alias Lazypock.Files.Store

  @doc """
  POST /api/files
  Upload a file (multipart/form-data).
  """
  def upload(conn, %{"file" => upload}) do
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
            |> put_resp_header("content-disposition", ~s(inline; filename="#{file_record["filename"]}"))
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
