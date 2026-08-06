defmodule LazypockWeb.FileController do
  use LazypockWeb, :controller

  alias Lazypock.Files.Store
  alias Lazypock.Collections.Registry

  @default_max_file_size 10 * 1024 * 1024

  @doc """
  GET /api/files
  List uploaded files (newest first), with optional filters.

  Query params:
    * `page` / `perPage` — pagination
    * `collectionName` — only files for a collection
    * `fieldName` — only files for a field
    * `mime` — only files whose mime_type starts with this prefix (e.g. `image/`)
  """
  def index(conn, params) do
    page = parse_int(params["page"], 1)
    per_page = parse_int(params["perPage"], 50)

    opts = [
      page: page,
      per_page: per_page,
      collection_name: blank_to_nil(params["collectionName"]),
      field_name: blank_to_nil(params["fieldName"]),
      mime: blank_to_nil(params["mime"])
    ]

    {:ok, %{items: items, page: page, per_page: per_page, total: total}} = Store.list(opts)

    conn
    |> put_status(200)
    |> json(%{
      "items" => Enum.map(items, &format_file/1),
      "page" => page,
      "perPage" => per_page,
      "total" => total
    })
  end

  defp parse_int(nil, default), do: default
  defp parse_int(s, default) when is_binary(s) do
    case Integer.parse(s) do
      {n, _} -> n
      :error -> default
    end
  end
  defp parse_int(_, default), do: default

  defp blank_to_nil(nil), do: nil
  defp blank_to_nil(s) when is_binary(s) and s == "", do: nil
  defp blank_to_nil(s), do: s


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
    size = file_size(upload)

    if size > max_size do
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
        thumb_sizes = resolve_thumb_sizes(conn.params)

        opts = %{
          collection_name: conn.params["collection_name"],
          record_id: conn.params["record_id"],
          field_name: conn.params["field_name"],
          thumb_sizes: thumb_sizes
        }

        case Store.store(upload, opts) do
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
  GET /api/files/:id/thumbs/:size
  Serve a generated thumbnail.
  """
  def show_thumb(conn, %{"id" => id, "size" => size}) do
    case Store.get(id) do
      {:ok, file_record} ->
        case file_record["thumbs"] do
          thumbs when is_map(thumbs) and map_size(thumbs) > 0 ->
            case Map.get(thumbs, size) do
              nil ->
                conn
                |> put_status(404)
                |> json(%{"code" => 404, "message" => "Thumbnail not found", "data" => %{}})

              thumb ->
                case Store.read_thumb(file_record, thumb) do
                  {:ok, binary} ->
                    conn
                    |> put_resp_header("content-type", thumb["mime_type"] || "image/webp")
                    |> send_resp(200, binary)

                  {:error, reason} ->
                    conn
                    |> put_status(500)
                    |> json(%{"code" => 500, "message" => inspect(reason), "data" => %{}})
                end
            end

          _ ->
            conn
            |> put_status(404)
            |> json(%{"code" => 404, "message" => "No thumbnails", "data" => %{}})
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

  # %Plug.Upload{} has path/content_type/filename but no size — derive it from disk.
  defp file_size(%Plug.Upload{} = upload) do
    case File.stat(upload.path) do
      {:ok, %File.Stat{size: size}} -> size
      {:error, _} -> 0
    end
  end

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

  # Resolve thumbnail sizes configured on the file field of the collection.
  defp resolve_thumb_sizes(params) do
    case params["collection_name"] do
      collection_name when is_binary(collection_name) and collection_name != "" ->
        with {:ok, collection} <- Registry.get(collection_name),
             fields <- collection.fields || [],
             file_field <- Enum.find(fields, fn f -> f.type in ~w(file multi_file) end),
             opts <- file_field.options || %{} do
          case opts["thumbs"] do
            thumbs when is_list(thumbs) -> thumbs
            _ -> []
          end
        else
          _ -> []
        end

      _ ->
        []
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
      "url" => Store.url(file_record),
      "thumbs" => normalize_thumbs(file_record["thumbs"], file_record["id"])
    }
  end

  # thumbs JSONB column is a map of {size => meta}; convert to a map of
  # {size => url} for clients, e.g. {"50x50" => "/api/files/<id>/thumbs/50x50"}.
  defp normalize_thumbs(thumbs, id) when is_map(thumbs) do
    Map.new(thumbs, fn {size, _meta} -> {size, "/api/files/#{id}/thumbs/#{size}"} end)
  end

  defp normalize_thumbs(_, _), do: %{}
end

