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
        inserted_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
        updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
      )
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

    case Ecto.Adapters.SQL.query(Repo, query, [id]) do
      {:ok, %{rows: [row], columns: cols}} ->
        {:ok, Enum.zip(cols, row) |> Map.new()}

      {:ok, _} ->
        {:error, :not_found}

      {:error, reason} ->
        {:error, reason}
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
      Ecto.Adapters.SQL.query(Repo,
        "SELECT storage_backend, storage_path FROM _files WHERE collection_name = $1 AND record_id = $2",
        [collection_name, to_string(record_id)]
      )

    # Delete physical files
    Enum.each(rows, fn [backend, storage_path] ->
      mod = Lazypock.Files.Adapter.for_backend(backend)
      mod.delete(%{"storage_backend" => backend, "storage_path" => storage_path})
    end)

    # Delete metadata
    Ecto.Adapters.SQL.query!(Repo,
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

        Ecto.Adapters.SQL.query!(Repo, "DELETE FROM _files WHERE id = $1", [file_record["id"]])
        :ok

      {:error, reason} ->
        {:error, reason}
    end
  end

  defp do_store(binary, filename, content_type, _opts) do
    # always local by default
    adapter_mod = Lazypock.Files.Adapters.Local

    case adapter_mod.store(binary, filename, []) do
      {:ok, meta} ->
        ext = Path.extname(filename)
        mime = content_type || meta[:mime_type]

        {:ok, %{rows: [[id]]}} =
          Ecto.Adapters.SQL.query(
            Repo,
            """
            INSERT INTO _files (filename, extension, mime_type, size, storage_path, storage_backend)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id
            """,
            [filename, ext, mime, meta[:size], meta[:path], "local"]
          )

        get(id)

      {:error, reason} ->
        {:error, reason}
    end
  end
end
