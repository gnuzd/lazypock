defmodule LazypockWeb.FileController do
  use LazypockWeb, :controller

  alias Lazypock.Files.Policy
  alias Lazypock.Files.Store
  alias Lazypock.Files.Validation
  alias Lazypock.Collections.Registry

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
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_index(conn, params)
  end

  defp do_index(conn, params) do
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

  Supports an optional `collection_name` field in the multipart body. The size,
  MIME and image-dimension limits are resolved by `Lazypock.Files.Policy`:
  field options first, then the global `upload` settings, then the default.
  """
  def upload(conn, %{"file" => upload}) do
    conn = require_authenticated!(conn)
    if conn.halted, do: conn, else: do_upload_with_size_check(conn, upload)
  end

  def upload(conn, _params) do
    conn = require_authenticated!(conn)

    if conn.halted do
      conn
    else
      conn
      |> put_status(400)
      |> json(%{"code" => 400, "message" => "Missing required field: file", "data" => %{}})
    end
  end

  defp do_upload_with_size_check(conn, upload) do
    field_options = file_field_options(conn.params)
    policy = Policy.resolve(field_options)
    max_size = Policy.max_size(policy)
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
      do_upload(conn, upload, policy, field_options)
    end
  end

  defp do_upload(conn, upload, policy, field_options) do
    # Validate straight from the temp file so image uploads are never buffered
    # in the BEAM; `validate_file/2` only reads a magic-byte prefix.
    case Validation.validate_file(upload.filename, upload.path) do
      {:error, reason} ->
        conn
        |> put_status(400)
        |> json(%{
          "code" => 400,
          "message" => upload_error_message(reason),
          "data" => %{}
        })

      {:ok, mime} ->
        cond do
          not Policy.mime_allowed?(mime, policy) ->
            conn
            |> put_status(400)
            |> json(%{
              "code" => 400,
              "message" => upload_error_message(:extension_not_allowed),
              "data" => %{}
            })

          true ->
            do_store_upload(conn, upload, policy, field_options, mime)
        end
    end
  end

  defp do_store_upload(conn, upload, policy, field_options, mime) do
    case check_dimensions(upload, mime, policy) do
      {:error, :overloaded} ->
        conn
        |> put_resp_header("retry-after", "2")
        |> put_status(503)
        |> json(%{
          "code" => 503,
          "message" => "Image queue is busy, retry shortly.",
          "data" => %{}
        })

      {:error, reason} ->
        conn
        |> put_status(422)
        |> json(%{"code" => 422, "message" => dimension_error_message(reason), "data" => %{}})

      _ok_or_unknown ->
        store_uploaded_file(conn, upload, field_options, mime)
    end
  end

  defp store_uploaded_file(conn, upload, field_options, mime) do
    opts = [
      collection_name: conn.params["collection_name"],
      record_id: conn.params["record_id"],
      field_name: conn.params["field_name"],
      thumb_sizes: resolve_thumb_sizes(field_options),
      variants: resolve_variants(conn.params),
      mime_type: mime
    ]

    # Stream from the temp file; the persisted MIME type is the
    # server-determined one from validation.
    case Store.store({:file, upload.path}, upload.filename, opts) do
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

  # The dimension guard only applies to images. `:unknown` (no ImageMagick)
  # lets the upload through — the size cap and limiter still apply.
  defp check_dimensions(upload, "image/" <> _rest, policy),
    do: Policy.check_image(upload.path, policy)

  defp check_dimensions(_upload, _mime, _policy), do: :ok

  defp dimension_error_message(:image_too_large),
    do: "Image dimensions exceed the maximum allowed."

  defp dimension_error_message(:image_too_many_pixels),
    do: "Image exceeds the maximum allowed megapixels."

  defp upload_error_message(reason) do
    case reason do
      :extension_not_allowed ->
        "File type not allowed. Upload an image, PDF, CSV, TXT, JSON, ZIP, MP4, or MP3 file."

      :content_mismatch ->
        "File contents do not match the filename extension."

      :invalid_text ->
        "Text uploads must be valid UTF-8 (and valid JSON for .json files)."

      _ ->
        "File type not allowed."
    end
  end

  @doc """
  GET /api/files/:id
  Serve a file.
  """
  def show(conn, %{"id" => id}) do
    # Fire onFileDownloadRequest (PocketBase parity)
    case Lazypock.Hooks.Request.trigger_file_download(conn, id, nil) do
      {:ok, _event} ->
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

      {:error, reason} ->
        conn
        |> put_status(400)
        |> json(%{"code" => 400, "message" => to_string(reason), "data" => %{}})
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
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_delete(conn, id)
  end

  defp do_delete(conn, id) do
    case Store.delete(id) do
      :ok ->
        conn |> put_status(204) |> json(nil)

      {:error, reason} ->
        conn
        |> put_status(400)
        |> json(%{"code" => 400, "message" => inspect(reason), "data" => %{}})
    end
  end

  # Upload is allowed for ANY authenticated identity (superuser or auth-collection
  # user) — app SDKs upload files with end-user tokens. Anonymous uploads are
  # rejected to prevent unauthenticated disk usage. Per-record rule enforcement
  # on uploads is an open item (the endpoint currently has no record context).
  defp require_authenticated!(conn) do
    case {conn.assigns[:current_superuser], conn.assigns[:current_user]} do
      {nil, nil} ->
        conn
        |> put_resp_content_type("application/json")
        |> send_resp(
          403,
          Jason.encode!(%{
            code: 403,
            message: "Access denied. Authentication required.",
            data: %{}
          })
        )
        |> halt()

      _ ->
        conn
    end
  end

  defp require_superuser!(conn) do
    case conn.assigns[:current_superuser] do
      nil ->
        conn
        |> put_resp_content_type("application/json")
        |> send_resp(
          403,
          Jason.encode!(%{code: 403, message: "Access denied. Superuser required.", data: %{}})
        )
        |> halt()

      _user ->
        conn
    end
  end

  @doc """
  GET /api/files/:id/scale/:size
  Serve an on-demand resized version of an image (cached).

  `:size` is an ImageMagick geometry: `100`, `100x`, `x100`, `100x200`, `100x200!`.
  """
  def show_scaled(conn, %{"id" => id, "size" => size}) do
    case Store.get(id) do
      {:ok, file_record} ->
        case Store.scale(file_record, size) do
          {:ok, binary, mime_type} ->
            conn
            |> put_resp_header("content-type", mime_type)
            |> put_resp_header("cache-control", "public, max-age=31536000, immutable")
            |> send_resp(200, binary)

          {:error, :overloaded} ->
            conn
            |> put_resp_header("retry-after", "2")
            |> put_status(503)
            |> json(%{
              "code" => 503,
              "message" => "Image queue is busy, retry shortly.",
              "data" => %{}
            })

          {:error, reason} ->
            conn
            |> put_status(400)
            |> json(%{"code" => 400, "message" => inspect(reason), "data" => %{}})
        end

      {:error, :not_found} ->
        conn
        |> put_status(404)
        |> json(%{"code" => 404, "message" => "File not found", "data" => %{}})

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

  defp file_field_options(params) do
    case params["collection_name"] do
      name when is_binary(name) and name != "" ->
        with {:ok, collection} <- Registry.get(name),
             fields <- collection.fields || [],
             %{} = field <- Enum.find(fields, fn f -> f.type in ~w(file multi_file) end) do
          field.options || %{}
        else
          _ -> %{}
        end

      _ ->
        %{}
    end
  end

  # Thumbnail sizes configured on the file field of the collection.
  defp resolve_thumb_sizes(field_options) do
    case field_options["thumbs"] do
      thumbs when is_list(thumbs) -> thumbs
      _ -> []
    end
  end

  # `?variants=thumb,content` asks for those preset variants to be generated
  # before responding, so the returned URLs are valid immediately.
  defp resolve_variants(params) do
    case params["variants"] do
      value when is_binary(value) ->
        value |> String.split(",") |> Enum.map(&String.trim/1) |> Enum.reject(&(&1 == ""))

      value when is_list(value) ->
        value

      _ ->
        []
    end
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
      "thumbs" => normalize_thumbs(file_record["thumbs"], file_record["id"]),
      "variants" => variant_urls(file_record)
    }
  end

  # Preset variant URLs, by convention `/api/files/:id/scale/:preset`. They are
  # generated eagerly on upload or lazily on first request.
  defp variant_urls(file_record) do
    if is_binary(file_record["mime_type"]) and
         String.starts_with?(file_record["mime_type"], "image/") do
      id = file_record["id"]

      Lazypock.Files.Presets.all()
      |> Map.new(fn preset -> {preset["name"], "/api/files/#{id}/scale/#{preset["name"]}"} end)
    else
      %{}
    end
  end

  # thumbs JSONB column is a map of {size => meta}; convert to a map of
  # {size => url} for clients, e.g. {"50x50" => "/api/files/<id>/thumbs/50x50"}.
  defp normalize_thumbs(thumbs, id) when is_map(thumbs) do
    Map.new(thumbs, fn {size, _meta} -> {size, "/api/files/#{id}/thumbs/#{size}"} end)
  end

  defp normalize_thumbs(_, _), do: %{}
end
