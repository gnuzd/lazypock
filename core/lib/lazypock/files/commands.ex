defmodule Lazypock.Files.Commands do
  @moduledoc """
  Operational commands for stored files, used by the `lazypock files` CLI:

    * `reap/0` — process the deletion outbox and stale pending uploads
    * `regen/1` — regenerate preset variants (`--preset NAME`)
    * `reconcile/1` — report/delete orphaned local objects (`--dry-run`)
    * `migrate_to_s3/1` — copy local objects to S3 (`--delete-local`, `--dry-run`)
    * `trim_cache/1` — enforce the variant cache size cap

  These run with the repo started (see `Lazypock.Application`), so they work both
  from the release binary and from tests.
  """

  require Logger

  alias Lazypock.Files.Adapter
  alias Lazypock.Files.Presets
  alias Lazypock.Files.Reaper
  alias Lazypock.Files.Storage
  alias Lazypock.Files.Store
  alias Lazypock.Repo

  # ── Reap ─────────────────────────────────────────────

  def reap do
    Reaper.reap_stale_pending()

    Reaper.drain(1000)
    |> then(fn :ok -> :ok end)
  end

  # ── Regenerate variants ──────────────────────────────

  @doc "Regenerates preset variants. `opts[:preset]` limits it to one preset."
  def regen(opts \\ []) do
    presets =
      case opts[:preset] do
        nil -> Presets.all()
        name -> Enum.filter([Presets.get(name)], & &1)
      end

    rows = files_with_mime("image/%")

    Enum.reduce(rows, %{generated: 0, failed: 0}, fn record, acc ->
      adapter = Adapter.for_backend(record["storage_backend"])

      Enum.reduce(presets, acc, fn preset, acc ->
        purge_variant(record, preset)

        case adapter.scale(record, preset) do
          {:ok, _binary, _mime} -> Map.update!(acc, :generated, &(&1 + 1))
          {:error, _} -> Map.update!(acc, :failed, &(&1 + 1))
        end
      end)
    end)
  end

  # Local variants are cached on disk; delete the cached file so `scale/2`
  # regenerates it. Remote variants are left alone (delete the object first).
  defp purge_variant(%{"storage_backend" => "local"} = record, preset) do
    id = record["id"] |> to_string() |> String.replace("-", "")
    File.rm(Path.join([lazypock_upload_dir(), "_variants", id, "#{preset["name"]}.webp"]))
  end

  defp purge_variant(_record, _preset), do: :ok

  # ── Reconcile local storage ──────────────────────────

  @doc """
  Finds local files that no `_files` row references (orphans) and, unless
  `opts[:dry_run]`, deletes those older than the grace period (default 24 h).
  """
  def reconcile(opts \\ []) do
    base = lazypock_upload_dir()
    referenced = referenced_paths()
    grace_seconds = opts[:grace_seconds] || 24 * 60 * 60

    orphans =
      base
      |> list_files(["_cache", "_variants"])
      |> Enum.reject(&(Path.basename(&1) |> String.ends_with?(".tmp")))
      |> Enum.map(fn path -> {path, Path.relative_to(path, base)} end)
      |> Enum.reject(fn {_path, rel} -> MapSet.member?(referenced, rel) end)
      |> Enum.filter(fn {path, _rel} -> old_enough?(path, grace_seconds) end)

    if opts[:dry_run] do
      %{orphans: Enum.map(orphans, &elem(&1, 1)), deleted: 0, dry_run: true}
    else
      Enum.each(orphans, fn {path, _rel} -> File.rm(path) end)
      %{orphans: Enum.map(orphans, &elem(&1, 1)), deleted: length(orphans), dry_run: false}
    end
  end

  defp referenced_paths do
    {:ok, %{rows: rows}} =
      Ecto.Adapters.SQL.query(Repo, "SELECT storage_path, thumbs FROM _files", [])

    Enum.reduce(rows, MapSet.new(), fn [storage_path, thumbs], acc ->
      acc = MapSet.put(acc, storage_path)

      thumbs
      |> decode_json()
      |> Map.values()
      |> Enum.reduce(acc, fn meta, acc ->
        case meta do
          %{"path" => path} -> MapSet.put(acc, path)
          _ -> acc
        end
      end)
    end)
  end

  defp decode_json(nil), do: %{}

  defp decode_json(value) when is_map(value), do: value

  defp decode_json(value) when is_binary(value) do
    case Jason.decode(value) do
      {:ok, map} -> map
      _ -> %{}
    end
  end

  defp old_enough?(path, grace_seconds) do
    case File.stat(path, time: :posix) do
      {:ok, %File.Stat{mtime: mtime}} -> System.system_time(:second) - mtime > grace_seconds
      _ -> false
    end
  end

  defp list_files(base, skip \\ []) do
    case File.ls(base) do
      {:ok, entries} ->
        Enum.flat_map(entries, fn entry ->
          if entry in skip do
            []
          else
            path = Path.join(base, entry)
            if File.dir?(path), do: list_files(path, skip), else: [path]
          end
        end)

      _ ->
        []
    end
  end

  defp lazypock_upload_dir do
    Application.get_env(:lazypock, :file_storage)[:path] ||
      Path.join(Application.app_dir(:lazypock, "priv"), "uploads")
  end

  # ── Migrate local → S3 ───────────────────────────────

  @doc """
  Copies local objects to the configured S3/R2 bucket and repoints the rows.

  `opts[:dry_run]` reports what would move; `opts[:delete_local]` removes the
  local file after the copy is verified.
  """
  def migrate_to_s3(opts \\ []) do
    if not Storage.s3?() do
      {:error, :s3_not_configured}
    else
      rows = files_with_mime(nil, "local")

      result =
        Enum.reduce(rows, %{migrated: 0, failed: 0, skipped: 0}, fn record, acc ->
          if opts[:dry_run] do
            Map.update!(acc, :skipped, &(&1 + 1))
          else
            migrate_row(record, acc, opts[:delete_local] == true)
          end
        end)

      {:ok, result}
    end
  end

  defp migrate_row(record, acc, delete_local) do
    adapter = Lazypock.Files.Adapter.for_backend("local")

    with {:ok, local_path} <- adapter.local_path(record) do
      key = Lazypock.Files.Adapters.S3.object_key(record["id"], record["filename"])

      case Lazypock.Files.Adapters.S3.put_at(key, {:file, local_path},
             mime_type: record["mime_type"]
           ) do
        :ok ->
          Ecto.Adapters.SQL.query!(
            Repo,
            "UPDATE _files SET storage_backend = 's3', storage_path = $2, updated_at = now() WHERE id = $1",
            [Ecto.UUID.dump!(record["id"]), key]
          )

          if delete_local, do: File.rm(local_path)
          Map.update!(acc, :migrated, &(&1 + 1))

        {:error, reason} ->
          Logger.warning("Could not migrate #{record["id"]}: #{inspect(reason)}")
          Map.update!(acc, :failed, &(&1 + 1))
      end
    end
  end

  # ── Cache trim ───────────────────────────────────────

  @doc """
  Deletes the oldest cached variants/scale files until the cache is under
  `opts[:max_bytes]` (default `LAZYPOCK_VARIANT_CACHE_MAX` or 5 GB).
  """
  def trim_cache(opts \\ []) do
    max = opts[:max_bytes] || cache_max_bytes()
    base = Path.join(lazypock_upload_dir(), "_cache")
    variants = Path.join(lazypock_upload_dir(), "_variants")

    files = list_files(base) ++ list_files(variants)

    entries =
      files
      |> Enum.uniq()
      |> Enum.map(fn path ->
        case File.stat(path, time: :posix) do
          {:ok, %File.Stat{size: size, mtime: mtime}} -> {path, size, mtime}
          _ -> nil
        end
      end)
      |> Enum.reject(&is_nil/1)

    total = Enum.sum(Enum.map(entries, &elem(&1, 1)))

    if total <= max do
      %{deleted: 0, freed_bytes: 0, total_bytes: total}
    else
      {deleted, freed} =
        entries
        |> Enum.sort_by(&elem(&1, 2))
        |> Enum.reduce_while({0, 0}, fn {path, size, _mtime}, {count, freed} ->
          File.rm(path)

          if total - freed - size <= max do
            {:halt, {count + 1, freed + size}}
          else
            {:cont, {count + 1, freed + size}}
          end
        end)

      %{deleted: deleted, freed_bytes: freed, total_bytes: total}
    end
  end

  defp cache_max_bytes do
    case System.get_env("LAZYPOCK_VARIANT_CACHE_MAX") do
      nil -> 5 * 1024 * 1024 * 1024
      value -> parse_bytes(value)
    end
  end

  defp parse_bytes(value) do
    case Integer.parse(String.trim(value)) do
      {n, _} -> n
      :error -> 5 * 1024 * 1024 * 1024
    end
  end

  # ── Shared ───────────────────────────────────────────

  defp files_with_mime(like, backend \\ nil) do
    {where, args} =
      cond do
        like && backend -> {"WHERE mime_type LIKE $1 AND storage_backend = $2", [like, backend]}
        like -> {"WHERE mime_type LIKE $1", [like]}
        backend -> {"WHERE storage_backend = $1", [backend]}
        true -> {"", []}
      end

    {:ok, %{rows: rows, columns: cols}} =
      Ecto.Adapters.SQL.query(Repo, "SELECT * FROM _files #{where} ORDER BY created_at", args)

    Enum.map(rows, fn row ->
      cols
      |> Enum.zip(row)
      |> Map.new()
      |> normalize()
    end)
  end

  # `Store.get/1` normalisation is private; keep the parts the adapters need.
  defp normalize(record) do
    record
    |> Map.update("id", nil, fn id ->
      if is_binary(id) and byte_size(id) == 16 do
        case Ecto.UUID.load(id) do
          {:ok, str} -> str
          :error -> id
        end
      else
        id
      end
    end)
    |> Map.update("thumbs", %{}, fn
      value when is_binary(value) -> decode_json(value)
      value -> value
    end)
  end

  # Used by the CLI to reload a single row through the public API.
  @doc false
  def fetch(id), do: Store.get(id)
end
