defmodule Lazypock.Files.ReaperTest do
  use Lazypock.DataCase, async: false

  alias Lazypock.Files.Adapter
  alias Lazypock.Files.Reaper
  alias Lazypock.Files.Store
  alias Lazypock.Repo

  defp outbox_rows do
    {:ok, %{rows: rows}} =
      Ecto.Adapters.SQL.query(
        Repo,
        "SELECT file_id::text, backend, storage_path, attempts FROM _file_deletions ORDER BY id",
        []
      )

    rows
  end

  defp delete_row!(id) do
    Ecto.Adapters.SQL.query!(Repo, "DELETE FROM _files WHERE id = $1", [Ecto.UUID.dump!(id)])
  end

  test "a raw row delete enqueues an outbox row via the trigger" do
    {:ok, file} = Store.store("bye", "note.txt", [])

    delete_row!(file["id"])

    assert [[_file_id, "local", _path, 0]] = outbox_rows()
    assert {:error, :not_found} = Store.get(file["id"])
  end

  test "drain removes the stored object and the outbox row" do
    {:ok, file} = Store.store("bye", "note.txt", [])
    assert {:ok, _} = Store.read(file)

    delete_row!(file["id"])
    assert outbox_rows() != []

    assert :ok = Reaper.drain()
    assert outbox_rows() == []
    assert {:error, :enoent} = Store.read(file)
  end

  test "a missing object still clears the outbox row" do
    {:ok, file} = Store.store("bye", "note.txt", [])
    # Remove the physical file first; the reaper must treat 404/enoent as success.
    {:ok, path} = Adapter.for_backend("local").local_path(file)
    File.rm!(path)

    delete_row!(file["id"])
    assert :ok = Reaper.drain()
    assert outbox_rows() == []
  end

  test "Store.delete/1 and delete_by_record/2 enqueue and the reaper cleans up" do
    {:ok, f1} = Store.store("a", "a.txt", collection_name: "posts", record_id: "r1")
    {:ok, f2} = Store.store("b", "b.txt", collection_name: "posts", record_id: "r1")
    {:ok, f3} = Store.store("c", "c.txt", collection_name: "posts", record_id: "r2")

    assert :ok = Store.delete(f1["id"])
    assert :ok = Store.delete_by_record("posts", "r1")

    assert {:error, :not_found} = Store.get(f2["id"])
    assert {:ok, _} = Store.get(f3["id"])

    Reaper.drain()
    assert outbox_rows() == []
    assert {:error, :enoent} = Store.read(f2)

    Store.delete(f3["id"])
    Reaper.drain()
  end

  test "a failing adapter keeps the row and backs off" do
    file_id = Ecto.UUID.generate()

    Ecto.Adapters.SQL.query!(
      Repo,
      """
      INSERT INTO _file_deletions (file_id, backend, storage_path)
      VALUES ($1, 's3', 'some/key.txt')
      """,
      [Ecto.UUID.dump!(file_id)]
    )

    assert :ok = Reaper.drain()

    assert [[^file_id, "s3", "some/key.txt", 1]] = outbox_rows()

    {:ok, %{rows: [[next_try]]}} =
      Ecto.Adapters.SQL.query(
        Repo,
        "SELECT next_try_at > now() FROM _file_deletions WHERE file_id = $1",
        [Ecto.UUID.dump!(file_id)]
      )

    assert next_try == true
  end

  test "stats/0 reports queue depth" do
    {:ok, file} = Store.store("bye", "note.txt", [])
    delete_row!(file["id"])

    stats = Reaper.stats()
    assert stats.pending >= 1
    assert stats.oldest_seconds >= 0
  end
end
