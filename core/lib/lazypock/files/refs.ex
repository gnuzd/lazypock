defmodule Lazypock.Files.Refs do
  @moduledoc """
  Tracks which records reference a file through a richtext/editor field.

  Richtext content is plain Markdown containing file URLs, so a reference is
  extracted from the UUIDs that appear in the field value. `sync_record/3` is
  called on record create/update, `delete_record/2` on delete.

  This makes it possible to:

    * refuse `DELETE /api/files/:id` while a record still uses the file
      (`409` + usage list, `?force=true` for superusers)
    * garbage-collect editor uploads that were never inserted into a record
      (`attached_at IS NULL`, `origin = "editor"`)
    * rewrite embedded URLs when the CDN domain or backend changes
  """

  alias Lazypock.Collections.Registry
  alias Lazypock.Repo
  alias Lazypock.Schema.TypeMapper

  @uuid_re ~r/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/i

  def ensure_table! do
    Ecto.Adapters.SQL.query!(
      Repo,
      """
      CREATE TABLE IF NOT EXISTS _file_refs (
        file_id    UUID NOT NULL,
        collection TEXT NOT NULL,
        record_id  TEXT NOT NULL,
        field      TEXT NOT NULL,
        PRIMARY KEY (file_id, collection, record_id, field)
      )
      """,
      []
    )

    Ecto.Adapters.SQL.query!(
      Repo,
      "CREATE INDEX IF NOT EXISTS _file_refs_record_idx ON _file_refs (collection, record_id)",
      []
    )

    Ecto.Adapters.SQL.query!(
      Repo,
      "CREATE INDEX IF NOT EXISTS _file_refs_file_idx ON _file_refs (file_id)",
      []
    )

    :ok
  end

  @doc "File ids (lowercased UUID strings) referenced by a Markdown value."
  def extract(markdown) when is_binary(markdown) do
    @uuid_re
    |> Regex.scan(markdown)
    |> List.flatten()
    |> Enum.map(&String.downcase/1)
    |> Enum.uniq()
  end

  def extract(_), do: []

  @doc """
  Reconciles the stored references for a record against its current editor
  fields. Returns `%{added: n, removed: n}`.
  """
  def sync_record(collection_name, record_id, record) do
    record_id = to_string(record_id)
    desired = desired_refs(collection_name, record)
    existing = existing_refs(collection_name, record_id)

    removed = existing -- desired
    added = desired -- existing

    Enum.each(removed, fn {file_id, field} ->
      delete_one!(file_id, collection_name, record_id, field)
    end)

    Enum.each(added, fn {file_id, field} ->
      insert_one!(file_id, collection_name, record_id, field)
    end)

    mark_attached!(Enum.map(added, &elem(&1, 0)))

    %{added: length(added), removed: length(removed)}
  end

  @doc "Removes every reference owned by a record."
  def delete_record(collection_name, record_id) do
    Ecto.Adapters.SQL.query!(
      Repo,
      "DELETE FROM _file_refs WHERE collection = $1 AND record_id = $2",
      [collection_name, to_string(record_id)]
    )

    :ok
  end

  @doc "Records/fields that reference a file (for the delete guard)."
  def usage(file_id) do
    {:ok, %{rows: rows}} =
      Ecto.Adapters.SQL.query(
        Repo,
        "SELECT collection, record_id, field FROM _file_refs WHERE file_id = $1 ORDER BY collection, record_id, field",
        [Ecto.UUID.dump!(file_id)]
      )

    Enum.map(rows, fn [collection, record_id, field] ->
      %{"collection" => collection, "recordId" => record_id, "field" => field}
    end)
  rescue
    _ -> []
  end

  @doc "Whether any record references the file."
  def referenced?(file_id), do: usage(file_id) != []

  @doc """
  Deletes editor uploads that were never attached to a record and are older than
  `ttl_seconds` (default `files.unattached_ttl`, 24 h). Library uploads
  (`origin = "library"`) and field-owned files are never auto-deleted.
  """
  def gc_unattached(ttl_seconds \\ nil) do
    ttl = ttl_seconds || unattached_ttl_seconds()

    {:ok, %{num_rows: count}} =
      Ecto.Adapters.SQL.query(
        Repo,
        """
        DELETE FROM _files
        WHERE origin = 'editor'
          AND attached_at IS NULL
          AND created_at < clock_timestamp() - make_interval(secs => $1)
        """,
        [ttl]
      )

    count
  end

  @doc """
  Rewrites embedded file URLs in every editor field, e.g. after moving to a CDN:

      lazypock content rewrite-urls --from /api/files --to https://cdn.example.com

  Applying it twice is a no-op. `opts[:dry_run]` only counts the matches.
  """
  def rewrite_urls(from, to, opts \\ []) do
    dry_run = opts[:dry_run] == true
    pattern = "%" <> from <> "%"

    Registry.list()
    |> Enum.flat_map(fn collection ->
      (collection.fields || [])
      |> Enum.filter(&(&1.type == "editor"))
      |> Enum.map(&{collection.name, &1.name})
    end)
    |> Enum.reduce(%{updated: 0, matched: 0, dry_run: dry_run}, fn {collection, field}, acc ->
      table = TypeMapper.quote_ident(collection)
      column = TypeMapper.quote_ident(field)

      if dry_run do
        Map.update!(acc, :matched, &(&1 + count_matches(table, column, pattern)))
      else
        {:ok, %{num_rows: count}} =
          Ecto.Adapters.SQL.query(
            Repo,
            "UPDATE #{table} SET #{column} = replace(#{column}, $1, $2) WHERE #{column} LIKE $3",
            [from, to, pattern]
          )

        Map.update!(acc, :updated, &(&1 + count))
      end
    end)
  end

  # ── Internals ────────────────────────────────────────

  defp desired_refs(collection_name, record) do
    collection_name
    |> editor_fields()
    |> Enum.flat_map(fn field ->
      record
      |> Map.get(field)
      |> extract()
      |> Enum.map(&{&1, field})
    end)
    |> Enum.uniq()
  end

  defp editor_fields(collection_name) do
    case Registry.get(collection_name) do
      {:ok, collection} ->
        (collection.fields || [])
        |> Enum.filter(&(&1.type == "editor"))
        |> Enum.map(& &1.name)

      _ ->
        []
    end
  end

  defp existing_refs(collection_name, record_id) do
    {:ok, %{rows: rows}} =
      Ecto.Adapters.SQL.query(
        Repo,
        "SELECT file_id::text, field FROM _file_refs WHERE collection = $1 AND record_id = $2",
        [collection_name, record_id]
      )

    Enum.map(rows, fn [file_id, field] -> {String.downcase(file_id), field} end)
  end

  defp insert_one!(file_id, collection_name, record_id, field) do
    Ecto.Adapters.SQL.query!(
      Repo,
      """
      INSERT INTO _file_refs (file_id, collection, record_id, field)
      VALUES ($1, $2, $3, $4)
      ON CONFLICT DO NOTHING
      """,
      [Ecto.UUID.dump!(file_id), collection_name, record_id, field]
    )
  end

  defp delete_one!(file_id, collection_name, record_id, field) do
    Ecto.Adapters.SQL.query!(
      Repo,
      "DELETE FROM _file_refs WHERE file_id = $1 AND collection = $2 AND record_id = $3 AND field = $4",
      [Ecto.UUID.dump!(file_id), collection_name, record_id, field]
    )
  end

  defp mark_attached!([]), do: :ok

  defp mark_attached!(file_ids) do
    ids = file_ids |> Enum.uniq() |> Enum.map(&Ecto.UUID.dump!/1)

    Ecto.Adapters.SQL.query!(
      Repo,
      "UPDATE _files SET attached_at = COALESCE(attached_at, now()) WHERE id = ANY($1::uuid[])",
      [ids]
    )
  end

  defp count_matches(table, column, pattern) do
    {:ok, %{rows: [[count]]}} =
      Ecto.Adapters.SQL.query(
        Repo,
        "SELECT COUNT(*)::int FROM #{table} WHERE #{column} LIKE $1",
        [pattern]
      )

    count
  end

  defp unattached_ttl_seconds do
    case Lazypock.Settings.get("files", %{}) do
      %{"unattached_ttl_ms" => ms} when is_integer(ms) and ms > 0 -> div(ms, 1000)
      _ -> 24 * 60 * 60
    end
  rescue
    _ -> 24 * 60 * 60
  end
end
