defmodule Lazypock.BackupRollbackTest do
  use Lazypock.DataCase, async: false

  alias Lazypock.Backup
  alias Lazypock.Collections.Registry
  alias Lazypock.Schema.DDL
  alias Lazypock.Schemas.GenericRecord

  defp cname(prefix), do: "#{prefix}_#{System.unique_integer([:positive]) |> abs()}"

  setup do
    Ecto.Adapters.SQL.query!(Repo, "DELETE FROM _import_snapshots", [])
    :ok
  end

  describe "atomic restore (default)" do
    test "rolls the whole batch back when one collection is malformed" do
      good = cname("good")
      bad = cname("bad")

      payload = %{
        "collections" => [
          %{
            "name" => good,
            "type" => "base",
            "schema" => [%{"name" => "title", "type" => "text"}]
          },
          # No "schema"/"fields" — the importer rejects this one.
          %{"name" => bad, "type" => "base"}
        ]
      }

      assert %{imported: [], errors: errors, rolled_back: true} = Backup.restore(payload)
      assert Enum.any?(errors, &(&1.name == bad))

      # The good collection must not survive the rollback, and the registry
      # cache must have been resynced (no manual reload! here on purpose).
      assert Registry.get(good) == {:error, :not_found}
      assert Repo.get_by(Lazypock.Collections.Collection, name: good) == nil
    end

    test "keeps a successful batch and records an undo snapshot" do
      name = cname("keep")
      payload = %{"collections" => [%{"name" => name, "type" => "base", "schema" => []}]}

      assert %{errors: [], rolled_back: false} = Backup.restore(payload)
      assert %{created_at: _} = Backup.last_snapshot()
    end

    test "the best-effort escape hatch still applies valid collections" do
      good = cname("good")
      bad = cname("bad")

      payload = %{
        "collections" => [
          %{"name" => good, "type" => "base", "schema" => []},
          %{"name" => bad, "type" => "base"}
        ]
      }

      assert %{imported: imported, errors: errors, rolled_back: false} =
               Backup.restore(payload, false, atomic: false)

      assert Enum.any?(imported, &(&1.name == good))
      assert Enum.any?(errors, &(&1.name == bad))
      assert Repo.get_by(Lazypock.Collections.Collection, name: good)
    end
  end

  describe "rollback/0" do
    test "reverts changed records and prunes records added since the snapshot" do
      name = cname("items")

      {:ok, _} =
        DDL.create_collection(name,
          type: "base",
          fields: [%{"name" => "title", "type" => "text"}]
        )

      {:ok, original} = GenericRecord.insert(name, %{"title" => "original"})

      payload = %{
        "collections" => [
          %{
            "name" => name,
            "type" => "base",
            "schema" => [%{"name" => "title", "type" => "text"}],
            "records" => [
              %{"id" => original["id"], "title" => "changed"},
              %{"id" => Ecto.UUID.generate(), "title" => "added"}
            ]
          }
        ]
      }

      assert %{errors: [], rolled_back: false} = Backup.restore(payload)
      assert length(GenericRecord.all(name)) == 2

      assert {:ok, %{errors: []}} = Backup.rollback()

      records = GenericRecord.all(name)
      assert [%{"title" => "original"}] = records
    end

    test "drops collections created by the import" do
      before = cname("before")
      created = cname("created")

      {:ok, _} = DDL.create_collection(before, type: "base", fields: [])

      payload = %{
        "collections" => [
          %{"name" => before, "type" => "base", "schema" => []},
          %{"name" => created, "type" => "base", "schema" => []}
        ]
      }

      assert %{errors: []} = Backup.restore(payload)
      assert {:ok, _} = Registry.get(created)

      assert {:ok, _} = Backup.rollback()

      assert Registry.get(created) == {:error, :not_found}
      assert {:ok, _} = Registry.get(before)
    end

    test "restores a collection the import dropped via delete_missing" do
      keep = cname("keep")
      victim = cname("victim")

      {:ok, _} = DDL.create_collection(keep, type: "base", fields: [])
      {:ok, _} = DDL.create_collection(victim, type: "base", fields: [])
      {:ok, victim_record} = GenericRecord.insert(victim, %{})

      payload = %{"collections" => [%{"name" => keep, "type" => "base", "schema" => []}]}

      assert %{errors: []} = Backup.restore(payload, true)
      assert Registry.get(victim) == {:error, :not_found}

      assert {:ok, _} = Backup.rollback()

      assert {:ok, _} = Registry.get(victim)
      assert [%{"id" => id}] = GenericRecord.all(victim)
      assert id == victim_record["id"]
    end

    test "leaves system collections untouched" do
      before_ids = GenericRecord.all("_superusers") |> Enum.map(& &1["id"]) |> Enum.sort()

      name = cname("user_coll")
      payload = %{"collections" => [%{"name" => name, "type" => "base", "schema" => []}]}

      assert %{errors: []} = Backup.restore(payload)
      assert {:ok, _} = Backup.rollback()

      after_ids = GenericRecord.all("_superusers") |> Enum.map(& &1["id"]) |> Enum.sort()
      assert before_ids == after_ids
    end

    test "returns :no_snapshot when there is nothing to undo" do
      assert {:error, :no_snapshot} = Backup.rollback()
    end

    test "consumes the snapshot so it cannot be rolled back twice" do
      name = cname("once")

      assert %{errors: []} =
               Backup.restore(%{
                 "collections" => [%{"name" => name, "type" => "base", "schema" => []}]
               })

      assert {:ok, _} = Backup.rollback()
      assert {:error, :no_snapshot} = Backup.rollback()
    end
  end
end
