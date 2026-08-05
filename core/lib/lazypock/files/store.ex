defmodule Lazypock.Files.Store do
  @moduledoc """
  File storage operations — upload, serve, delete.

  Delegates to the configured adapter based on _files.storage_backend.
  Default is always local (zero config, like PocketBase).
  """

  alias Lazypock.Repo

  @doc """
  Ensures the `_files` table exists on boot.
  """
  def ensure_files_table! do
    Ecto.Adapters.SQL.query!(
      Repo,
      """
      CREATE TABLE IF NOT EXISTS _files (
        id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
        filename        TEXT NOT NULL,
        extension       TEXT NOT NULL DEFAULT '',
        mime_type       TEXT NOT NULL DEFAULT 'application/octet-stream',
        size            BIGINT NOT NULL DEFAULT 0,
        storage_path    TEXT NOT NULL,
        storage_backend TEXT NOT NULL DEFAULT 'local',
        collection_name TEXT DEFAULT '',
        record_id       TEXT DEFAULT '',
        field_name      TEXT DEFAULT '',
        field_name      TEXT DEFAULT '',
        thumbs          JSONB DEFAULT '{}'::jsonb,
        created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
        updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
      )
      """,
      []
    )

    # Add thumbs column to existing tables (idempotent)
    Ecto.Adapters.SQL.query!(
      Repo,
      """
      ALTER TABLE _files ADD COLUMN IF NOT EXISTS thumbs JSONB DEFAULT '{}'::jsonb
      """,
      []
    )
  end

  @doc """
  Stores a file from a Plug.Upload struct or binary.

  ## Options

    * `:collection_name` — the collection this file belongs to (for cleanup on record delete)
    * `:record_id` — the record ID this file belongs to
    * `:field_name` — the field name on the record
  """
  def store(%Plug.Upload{} = upload, opts \\ []) do
    binary = File.read!(upload.path)
    do_store(binary, upload.filename, upload.content_type, opts)
  end

  def store(binary, filename, opts) when is_binary(binary) do
    do_store(binary, filename, nil, opts)
  end

  @doc """
  Returns a file record by ID.
  """
  def get(id) do
    query = "SELECT * FROM _files WHERE id = $1::uuid"

    case Ecto.Adapters.SQL.query(Repo, query, [to_uuid_binary(id)]) do
      {:ok, %{rows: [row], columns: cols}} ->
        {:ok, cols |> Enum.zip(row) |> Map.new() |> normalize_uuid() |> normalize_thumbs()}

      {:ok, _} ->
        {:error, :not_found}

      {:error, reason} ->
        {:error, reason}
    end
  end

  # Accept a UUID as string ("…-…-…") or as Postgrex raw 16-byte binary, and
  # always hand the query the raw binary form Postgrex expects for uuid columns.
  defp to_uuid_binary(id) when is_binary(id) and byte_size(id) == 16, do: id
  defp to_uuid_binary(id) when is_binary(id), do: Ecto.UUID.dump!(id)
  defp to_uuid_binary(id), do: id

  # Postgrex returns UUID columns as 16-byte raw binaries; convert to strings
  # so consumers (JSON encoding, URL interpolation) get a readable UUID.
  defp normalize_uuid(file_record) do
    case Map.fetch(file_record, "id") do
      {:ok, id} when is_binary(id) and byte_size(id) == 16 ->
        case Ecto.UUID.load(id) do
          {:ok, str} -> Map.put(file_record, "id", str)
          :error -> file_record
        end

      _ ->
        file_record
    end
  end

  # Postgrex returns JSONB columns as JSON strings; decode to maps so
  # consumers (controller, adapter) get plain Elixir maps.
  defp normalize_thumbs(file_record) do
    case Map.fetch(file_record, "thumbs") do
      {:ok, thumbs} when is_binary(thumbs) ->
        case Jason.decode(thumbs) do
          {:ok, map} -> Map.put(file_record, "thumbs", map)
          _ -> file_record
        end

      _ ->
        file_record
    end
  end

  @doc """
  Returns the file binary.
  """
  def read(file_record) do
    mod = Lazypock.Files.Adapter.for_backend(file_record["storage_backend"])
    mod.get(file_record)
  end

  @doc """
  Reads a generated thumbnail binary for a file record.
  """
  def read_thumb(file_record, thumb) do
    mod = Lazypock.Files.Adapter.for_backend(file_record["storage_backend"])
    mod.thumb_get(file_record, thumb)
  end

  @doc """
  Returns the URL for a file.
  """
  def url(file_record) do
    mod = Lazypock.Files.Adapter.for_backend(file_record["storage_backend"])
    mod.url(file_record)
  end

  @doc """
  Deletes all files associated with a collection record.
  """
  def delete_by_record(collection_name, record_id) do
    {:ok, %{rows: rows}} =
      Ecto.Adapters.SQL.query(
        Repo,
        "SELECT storage_backend, storage_path FROM _files WHERE collection_name = $1 AND record_id = $2",
        [collection_name, to_string(record_id)]
      )

    # Delete physical files
    Enum.each(rows, fn [backend, storage_path] ->
      mod = Lazypock.Files.Adapter.for_backend(backend)
      mod.delete(%{"storage_backend" => backend, "storage_path" => storage_path})
    end)

    # Delete metadata
    Ecto.Adapters.SQL.query!(
      Repo,
      "DELETE FROM _files WHERE collection_name = $1 AND record_id = $2",
      [collection_name, to_string(record_id)]
    )

    :ok
  end

  @doc """
  Deletes a file record and its underlying storage.
  """
  def delete(id) do
    case get(id) do
      {:ok, file_record} ->
        mod = Lazypock.Files.Adapter.for_backend(file_record["storage_backend"])
        mod.delete(file_record)

        Ecto.Adapters.SQL.query!(Repo, "DELETE FROM _files WHERE id = $1", [to_uuid_binary(file_record["id"])])
        :ok

      {:error, reason} ->
        {:error, reason}
    end
  end

  defp do_store(binary, filename, content_type, opts) do
    # always local by default
    adapter_mod = Lazypock.Files.Adapters.Local

    case adapter_mod.store(binary, filename, []) do
      {:ok, meta} ->
        ext = Path.extname(filename)
        mime = content_type || meta[:mime_type]
        thumb_sizes = opts[:thumb_sizes] || []

        {:ok, %{rows: [[id]]}} =
          Ecto.Adapters.SQL.query(
            Repo,
            """
            INSERT INTO _files (filename, extension, mime_type, size, storage_path, storage_backend, collection_name, record_id, field_name)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING id
            """,
            [
              filename,
              ext,
              mime,
              meta[:size],
              meta[:path],
              "local",
              opts[:collection_name] || "",
              to_string(opts[:record_id] || ""),
              opts[:field_name] || ""
            ]
          )

        thumbs =
          if thumb_sizes != [] do
            safe_generate_thumbs(adapter_mod, binary, filename, thumb_sizes)
          else
            []
          end

        if thumbs != [] do
          thumbs_map = Map.new(thumbs, fn t -> {t["size"], t} end)
          thumbs_json = Jason.encode!(thumbs_map)

          Ecto.Adapters.SQL.query!(
            Repo,
            "UPDATE _files SET thumbs = $1::jsonb WHERE id = $2::uuid",
            [thumbs_json, to_uuid_binary(id)]
          )
        end

        get(id)

      {:error, reason} ->
        {:error, reason}
    end
  end

  # Thumbnail generation is best-effort: any failure (missing ImageMagick,
  # non-image file, resize error) must not break the upload.
  defp safe_generate_thumbs(adapter_mod, binary, filename, thumb_sizes) do
    {:ok, thumbs} = adapter_mod.thumbs(binary, filename, thumb_sizes)
    thumbs
  rescue
    _ -> []
  catch
    :exit, _ -> []
  end
end
