defmodule Lazypock.BackupArchiveTest do
  @moduledoc """
  Streaming NDJSON archive export/import (large-database backup & restore).

  Covers the parts that only exist because a single JSON document can no longer
  hold the database: cursor-based export, per-line NDJSON, batched upserts with
  the Postgres parameter cap, the three `:atomic` modes, and file-based undo
  checkpoints with a size threshold.
  """
  use Lazypock.DataCase, async: false

  alias Lazypock.Backup
  alias Lazypock.Collections.Registry
  alias Lazypock.Schema.DDL
  alias Lazypock.Schemas.GenericRecord

  defp cname(prefix), do: "#{prefix}_#{System.unique_integer([:positive]) |> abs()}"

  defp tmp_path(suffix) do
    Path.join(System.tmp_dir!(), "lz-test-#{System.unique_integer([:positive])}.#{suffix}")
  end

  # Unzips an archive into a scratch dir and returns it.
  defp unzip(path) do
    dir = Path.join(System.tmp_dir!(), "lz-unzip-#{System.unique_integer([:positive])}")
    File.mkdir_p!(dir)
    {:ok, _} = :zip.extract(String.to_charlist(path), [{:cwd, String.to_charlist(dir)}])
    dir
  end

  defp entries(path) do
    {:ok, list} = :zip.list_dir(String.to_charlist(path))

    Enum.map(list, fn
      {:zip_file, name, _, _, _, _} -> List.to_string(name)
      other -> other
    end)
  end

  defp make_collection(name, fields) do
    {:ok, _} = DDL.create_collection(name, fields: fields)
    Registry.reload!()
    name
  end

  setup do
    Ecto.Adapters.SQL.query!(Repo, "DELETE FROM _import_snapshots", [])

    on_exit(fn ->
      System.delete_env("LAZYPOCK_IMPORT_UNDO_MAX_MB")
      System.delete_env("LAZYPOCK_IMPORT_BATCH_SIZE")
    end)

    :ok
  end

  describe "export_stream/1" do
    test "writes manifest.json, schema.json and one NDJSON file per collection" do
      name = make_collection(cname("arc"), [%{"name" => "title", "type" => "text"}])
      {:ok, _} = GenericRecord.insert(name, %{"title" => "hello"})

      path = tmp_path("zip")

      assert {:ok, %{path: ^path, collections: collections, records: 1}} =
               Backup.export_stream(dest: path)

      assert collections >= 1
      assert File.exists?(path)

      files = entries(path)
      assert "manifest.json" in files
      assert "schema.json" in files
      assert "data/#{name}.ndjson" in files

      dir = unzip(path)
      manifest = Jason.decode!(File.read!(Path.join(dir, "manifest.json")))
      assert manifest["format"] == "lazypock-archive"
      assert manifest["format_version"] == 1

      schema = Jason.decode!(File.read!(Path.join(dir, "schema.json")))
      assert Enum.any?(schema["collections"], &(&1["name"] == name))

      # schema.json carries no records — they live in data/*.ndjson.
      refute Enum.any?(schema["collections"], &Map.has_key?(&1, "records"))

      lines =
        Path.join([dir, "data", "#{name}.ndjson"])
        |> File.read!()
        |> String.split("\n", trim: true)

      assert length(lines) == 1
      assert %{"title" => "hello"} = Jason.decode!(hd(lines))

      File.rm(path)
      File.rm_rf(dir)
    end

    test "a record much larger than the cursor batch is one NDJSON line, not buffered" do
      name = make_collection(cname("big"), [%{"name" => "body", "type" => "editor"}])
      big = String.duplicate("x", 2_000_000)
      {:ok, _} = GenericRecord.insert(name, %{"body" => big})

      path = tmp_path("zip")
      assert {:ok, %{records: 1}} = Backup.export_stream(dest: path, max_rows: 1)

      dir = unzip(path)
      raw = File.read!(Path.join([dir, "data", "#{name}.ndjson"]))

      # One line, containing the whole value (plus JSON overhead).
      assert length(String.split(raw, "\n", trim: true)) == 1
      assert byte_size(raw) > 2_000_000
      assert Jason.decode!(String.trim_trailing(raw, "\n"))["body"] == big

      File.rm(path)
      File.rm_rf(dir)
    end
  end

  describe "restore_archive/3 round-trip" do
    test "restores schemas and records from an archive" do
      name = make_collection(cname("rt"), [%{"name" => "title", "type" => "text"}])
      {:ok, inserted} = GenericRecord.insert(name, %{"title" => "before"})

      path = tmp_path("zip")
      assert {:ok, _} = Backup.export_stream(dest: path)

      # Wipe the data, then restore it from the archive.
      Ecto.Adapters.SQL.query!(Repo, "DELETE FROM \"#{name}\"", [])
      assert GenericRecord.all(name) == []

      result = Backup.restore_archive(path, false, snapshot: false)
      assert result.errors == []
      assert Enum.any?(result.imported, &(&1.name == name))

      records = GenericRecord.all(name)
      assert length(records) == 1
      assert hd(records)["id"] == inserted["id"]
      assert hd(records)["title"] == "before"

      File.rm(path)
    end

    test "restores a >2MB value byte-for-byte" do
      name = make_collection(cname("bigrt"), [%{"name" => "body", "type" => "editor"}])
      big = String.duplicate("abcdefghij", 250_000)
      {:ok, _} = GenericRecord.insert(name, %{"body" => big})

      path = tmp_path("zip")
      assert {:ok, _} = Backup.export_stream(dest: path)

      Ecto.Adapters.SQL.query!(Repo, "DELETE FROM \"#{name}\"", [])
      assert Backup.restore_archive(path, false, snapshot: false).errors == []
      assert hd(GenericRecord.all(name))["body"] == big

      File.rm(path)
    end

    test "rejects a file that is not a LazyPock archive" do
      path = tmp_path("zip")
      File.write!(path, "not a zip")

      result = Backup.restore_archive(path, false, snapshot: false)
      assert result.rolled_back
      assert [%{error: error}] = result.errors
      assert error =~ "could not read archive"

      File.rm(path)
    end
  end

  describe "batched upserts" do
    test "restores many records in batches" do
      name = make_collection(cname("many"), [%{"name" => "n", "type" => "number"}])

      records = for i <- 1..1_200, do: %{"n" => i}

      payload = %{
        "collections" => [
          %{"name" => name, "type" => "base", "schema" => [], "records" => records}
        ]
      }

      result = Backup.restore(payload, false, snapshot: false)
      assert result.errors == []
      assert [%{records_imported: 1_200}] = result.imported
      assert length(GenericRecord.all(name)) == 1_200
    end

    test "honours the Postgres bind-parameter cap on a wide collection" do
      # 140 fields + 1 id = 141 params/row. A naive 500-row batch would need
      # 70 500 params (> 65 535) and fail; the cap must shrink the batch instead.
      fields = for i <- 1..140, do: %{"name" => "f#{i}", "type" => "text"}
      name = make_collection(cname("wide"), fields)

      records =
        for n <- 1..600 do
          Map.new(1..140, fn i -> {"f#{i}", "v#{n}-#{i}"} end)
        end

      payload = %{
        "collections" => [
          %{"name" => name, "type" => "base", "schema" => [], "records" => records}
        ]
      }

      result = Backup.restore(payload, false, snapshot: false)
      assert result.errors == []
      assert [%{records_imported: 600}] = result.imported
      assert length(GenericRecord.all(name)) == 600
    end

    test "returns the real error when a batch cannot be written" do
      name = make_collection(cname("bad"), [%{"name" => "title", "type" => "text"}])

      payload = %{
        "collections" => [
          %{
            "name" => name,
            "type" => "base",
            "schema" => [],
            "records" => [%{"id" => Ecto.UUID.generate(), "no_such_column" => "x"}]
          }
        ]
      }

      result = Backup.restore(payload, false, snapshot: false)
      assert [%{error: error}] = result.errors
      refute error =~ "25P02"
      assert error =~ "no_such_column"
    end
  end

  describe "atomic modes" do
    defp mixed_payload(good, bad) do
      %{
        "collections" => [
          %{"name" => good, "type" => "base", "schema" => []},
          # No "schema"/"fields" — rejected by the importer.
          %{"name" => bad, "type" => "base"}
        ]
      }
    end

    test ":batch (default) rolls the whole import back" do
      good = cname("batchgood")
      bad = cname("batchbad")

      result = Backup.restore(mixed_payload(good, bad), false, snapshot: false)
      assert result.rolled_back
      assert result.imported == []
      assert Repo.get_by(Lazypock.Collections.Collection, name: good) == nil
    end

    test ":per_collection keeps the collections that succeeded" do
      good = cname("pergood")
      bad = cname("perbad")

      result =
        Backup.restore(mixed_payload(good, bad), false, snapshot: false, atomic: :per_collection)

      refute result.rolled_back
      assert Enum.any?(result.imported, &(&1.name == good))
      assert Enum.any?(result.errors, &(&1.name == bad))
      assert Repo.get_by(Lazypock.Collections.Collection, name: good)
    end

    test "true is still an alias for :batch" do
      assert Backup.atomic_mode(true) == :batch
      assert Backup.atomic_mode(false) == false
      assert Backup.atomic_mode(:per_collection) == :per_collection
    end
  end

  describe "undo checkpoints" do
    test "a successful import records a file-based checkpoint and rollback undoes it" do
      name = make_collection(cname("ckpt"), [%{"name" => "title", "type" => "text"}])
      {:ok, original} = GenericRecord.insert(name, %{"title" => "original"})

      # Import a record while asking for a checkpoint.
      new_id = Ecto.UUID.generate()

      payload = %{
        "collections" => [
          %{
            "name" => name,
            "type" => "base",
            "schema" => [],
            "records" => [%{"id" => new_id, "title" => "imported"}]
          }
        ]
      }

      assert Backup.restore(payload, false, snapshot: true).errors == []
      assert length(GenericRecord.all(name)) == 2

      snapshot = Backup.last_snapshot()
      assert snapshot.file_based
      assert snapshot.format == "lazypock-archive"
      assert snapshot.byte_size > 0

      # Rollback restores the pre-import state AND prunes the added record.
      assert {:ok, %{errors: []}} = Backup.rollback()

      remaining = GenericRecord.all(name)
      assert length(remaining) == 1
      assert hd(remaining)["id"] == original["id"]
      assert hd(remaining)["title"] == "original"

      # Consumed.
      assert {:error, :no_snapshot} = Backup.rollback()
    end

    test "no checkpoint is written when the database is above the threshold" do
      # 0 MB makes "above the threshold" deterministic without needing a large
      # fixture (and pg_total_relation_size on a compressible value would not be
      # large anyway).
      System.put_env("LAZYPOCK_IMPORT_UNDO_MAX_MB", "0")

      name = make_collection(cname("nockpt"), [%{"name" => "title", "type" => "text"}])
      {:ok, _} = GenericRecord.insert(name, %{"title" => "anything"})

      refute Backup.undo_available?()

      preflight = Backup.preflight()
      assert preflight.undo_available == false
      assert preflight.threshold_mb == 0
      assert preflight.db_size_bytes > 0

      # snapshot: true is honoured as "as requested, but skipped" — the import
      # must still succeed, just without an undo.
      payload = %{
        "collections" => [%{"name" => cname("after"), "type" => "base", "schema" => []}]
      }

      assert Backup.restore(payload, false, snapshot: true).errors == []
      assert Backup.last_snapshot() == nil
    end

    test "checkpoint retention prunes old pointers and their archives" do
      System.put_env("LAZYPOCK_IMPORT_UNDO_KEEP", "1")

      name = make_collection(cname("ret"), [%{"name" => "title", "type" => "text"}])

      for i <- 1..3 do
        payload = %{
          "collections" => [
            %{
              "name" => name,
              "type" => "base",
              "schema" => [],
              "records" => [%{"id" => Ecto.UUID.generate(), "title" => "v#{i}"}]
            }
          ]
        }

        assert Backup.restore(payload, false, snapshot: true).errors == []
      end

      {:ok, %{rows: rows}} =
        Ecto.Adapters.SQL.query(Repo, "SELECT COUNT(*) FROM _import_snapshots", [])

      assert [[1]] = rows

      System.delete_env("LAZYPOCK_IMPORT_UNDO_KEEP")
    end
  end

  describe "pruning" do
    test "rollback deletes records added since the checkpoint" do
      name = make_collection(cname("prune"), [%{"name" => "title", "type" => "text"}])
      {:ok, _} = GenericRecord.insert(name, %{"title" => "keep-me"})

      payload = %{
        "collections" => [
          %{
            "name" => name,
            "type" => "base",
            "schema" => [],
            "records" => [
              %{"id" => Ecto.UUID.generate(), "title" => "added-1"},
              %{"id" => Ecto.UUID.generate(), "title" => "added-2"}
            ]
          }
        ]
      }

      assert Backup.restore(payload, false, snapshot: true).errors == []
      assert length(GenericRecord.all(name)) == 3

      assert {:ok, _} = Backup.rollback()

      titles = GenericRecord.all(name) |> Enum.map(& &1["title"])
      assert titles == ["keep-me"]
    end
  end

  describe "configuration" do
    test "backup_dir honours LAZYPOCK_BACKUP_DIR" do
      dir = Path.join(System.tmp_dir!(), "lz-bdir-#{System.unique_integer([:positive])}")
      System.put_env("LAZYPOCK_BACKUP_DIR", dir)
      assert Backup.backup_dir() == dir
      System.delete_env("LAZYPOCK_BACKUP_DIR")
    end

    test "neon detection can be forced and silenced" do
      System.put_env("LAZYPOCK_NEON_HOSTED", "1")
      assert Backup.neon_hosted?()

      System.put_env("LAZYPOCK_NEON_NOTICE", "0")
      refute Backup.neon_hosted?()

      System.delete_env("LAZYPOCK_NEON_HOSTED")
      System.delete_env("LAZYPOCK_NEON_NOTICE")
    end

    test "preflight exposes everything a UI needs to warn" do
      preflight = Backup.preflight()

      assert is_integer(preflight.db_size_bytes)
      assert is_integer(preflight.db_size_mb)
      assert is_integer(preflight.threshold_mb)
      assert is_boolean(preflight.undo_available)
      assert is_boolean(preflight.neon_hosted)
    end
  end
end
