defmodule Lazypock.Files.DirectUpload do
  @moduledoc """
  Mode B: the client uploads straight to S3/R2, so the VPS never handles the
  bytes.

    1. `presign/2` checks the declared size and MIME type against the upload
       policy, creates a `_files(status="pending")` row with a server-generated
       id and key, and returns a presigned `PUT`. `content-type` and
       `content-length` are signed to fixed values because R2 has no
       presigned-POST `content-length-range`.
    2. `complete/2` verifies the object (`HEAD` size, download, magic bytes,
       image dimension caps), generates the eager variants and flips the row to
       `ready`. A failure deletes the object and the row so nothing is orphaned.

  Abandoned `pending` rows are reaped after `files.pending_ttl` (default 1 h).
  """

  alias Lazypock.Files.Adapters.Local
  alias Lazypock.Files.Adapters.S3
  alias Lazypock.Files.Policy
  alias Lazypock.Files.Presets
  alias Lazypock.Files.Storage
  alias Lazypock.Files.Store
  alias Lazypock.Files.Validation
  alias Lazypock.Repo

  @doc "Creates a pending row and presigned PUT for a direct upload."
  @spec presign(map(), map()) :: {:ok, map()} | {:error, atom() | tuple()}
  def presign(params, policy) do
    filename = params["filename"] || "file"
    size = to_int(params["size"])
    declared_mime = params["mime"]
    ext = filename |> Path.extname() |> String.downcase()
    mime = declared_mime || Local.mime_type_for(ext)

    cond do
      not Storage.s3?() ->
        {:error, :direct_upload_requires_s3}

      is_nil(size) or size <= 0 ->
        {:error, :invalid_size}

      size > Policy.max_size(policy) ->
        {:error, :too_large}

      not Policy.mime_allowed?(mime, policy) ->
        {:error, :invalid_mime_type}

      not Enum.any?(Validation.allowed_extensions(), fn {e, _} -> e == ext end) ->
        {:error, :invalid_extension}

      true ->
        do_presign(filename, ext, mime, size, params)
    end
  end

  defp do_presign(filename, ext, mime, size, params) do
    id = Ecto.UUID.generate()
    key = S3.object_key(id, filename)
    origin = if blank?(params["collection_name"]), do: "library", else: "field"

    with {:ok, upload} <- S3.presign_put(key, mime, size) do
      Ecto.Adapters.SQL.query!(
        Repo,
        """
        INSERT INTO _files (id, filename, extension, mime_type, size, storage_path, storage_backend, status, original_name, origin, collection_name, record_id, field_name)
        VALUES ($1, $2, $3, $4, $5, $6, 's3', 'pending', $7, $8, $9, $10, $11)
        """,
        [
          Ecto.UUID.dump!(id),
          filename,
          ext,
          mime,
          size,
          key,
          filename,
          origin,
          params["collection_name"] || "",
          to_string(params["record_id"] || ""),
          params["field_name"] || ""
        ]
      )

      {:ok,
       %{
         "id" => id,
         "key" => key,
         "method" => upload.method,
         "url" => upload.url,
         "headers" => upload.headers,
         "storage_backend" => "s3"
       }}
    end
  end

  @doc "Verifies an uploaded object and marks the file ready."
  @spec complete(String.t(), map()) :: {:ok, map()} | {:error, term()}
  def complete(id, policy) do
    case Store.get(id) do
      {:error, :not_found} ->
        {:error, :not_found}

      {:ok, %{"status" => "ready"} = record} ->
        {:ok, record}

      {:ok, record} ->
        case verify(record, policy) do
          {:ok, variants, dimensions} ->
            {:ok, mark_ready!(record, variants, dimensions)}

          {:error, reason} ->
            abandon(record)
            {:error, reason}
        end
    end
  end

  defp verify(record, policy) do
    key = record["storage_path"]
    expected = record["size"]

    with :ok <- verify_size(key, expected),
         {:ok, tmp} <- download(key) do
      try do
        with {:ok, _mime} <- Validation.validate_file(record["filename"], tmp),
             :ok <- verify_dimensions(tmp, policy) do
          {:ok, generate_variants(record), dimensions(tmp)}
        end
      after
        File.rm(tmp)
      end
    end
  end

  defp verify_size(key, expected) do
    case S3.head_size(key) do
      {:ok, nil} -> :ok
      {:ok, ^expected} -> :ok
      {:ok, actual} -> {:error, {:size_mismatch, actual, expected}}
      {:error, :not_found} -> {:error, :not_uploaded}
      {:error, reason} -> {:error, reason}
    end
  end

  defp download(key) do
    tmp = Path.join(System.tmp_dir!(), "lazypock-direct-#{Ecto.UUID.generate()}")

    case S3.download_to(key, tmp) do
      :ok -> {:ok, tmp}
      {:error, reason} -> {:error, reason}
    end
  end

  defp verify_dimensions(tmp, policy) do
    case Policy.check_image(tmp, policy) do
      :ok -> :ok
      :unknown -> :ok
      {:error, reason} -> {:error, reason}
    end
  end

  defp dimensions(tmp) do
    case Lazypock.Files.Limiter.run(fn -> Lazypock.Images.engine().dimensions(tmp) end) do
      {:ok, {:ok, w, h}} -> {w, h}
      _ -> {nil, nil}
    end
  end

  defp generate_variants(record) do
    adapter = Lazypock.Files.Adapter.for_backend("s3")

    Presets.eager()
    |> Enum.reduce(%{}, fn preset, acc ->
      case adapter.scale(record, preset) do
        {:ok, _binary, _mime} -> Map.put(acc, preset["name"], %{"mime_type" => "image/webp"})
        _ -> acc
      end
    end)
  end

  defp mark_ready!(record, variants, {width, height}) do
    Ecto.Adapters.SQL.query!(
      Repo,
      """
      UPDATE _files
      SET status = 'ready', variants = $2::jsonb, width = $3, height = $4, updated_at = now()
      WHERE id = $1
      """,
      [
        Ecto.UUID.dump!(record["id"]),
        Jason.encode!(variants),
        width,
        height
      ]
    )

    Map.merge(record, %{
      "status" => "ready",
      "variants" => variants,
      "width" => width,
      "height" => height
    })
  end

  # Verify failed: remove the object and the pending row.
  defp abandon(record) do
    S3.delete(record)

    Ecto.Adapters.SQL.query!(Repo, "DELETE FROM _files WHERE id = $1", [
      Ecto.UUID.dump!(record["id"])
    ])

    :ok
  end

  defp to_int(nil), do: nil
  defp to_int(value) when is_integer(value), do: value

  defp to_int(value) when is_binary(value) do
    case Integer.parse(value) do
      {n, _} -> n
      :error -> nil
    end
  end

  defp to_int(_), do: nil

  defp blank?(nil), do: true
  defp blank?(""), do: true

  defp blank?(value) when is_binary(value), do: String.trim(value) == ""
  defp blank?(_), do: true
end
