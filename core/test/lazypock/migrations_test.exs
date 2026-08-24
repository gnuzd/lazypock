defmodule Lazypock.MigrationsTest do
  use Lazypock.DataCase, async: false

  alias Lazypock.Collections.Registry
  alias Lazypock.Migrations

  # User migrations create REAL collections through these helpers — a raw
  # `create table` would never register `_collections`/`_fields` rows, which
  # is exactly why tables created by raw migrations don't show up in the
  # Studio. These tests pin the helper contract.
  describe "collection helpers (callable from user migrations / seeds)" do
    test "create_collection/2 registers a collection visible to the registry" do
      name = "mig_posts_#{:erlang.unique_integer([:positive])}"

      assert {:ok, _collection} =
               Migrations.create_collection(name,
                 type: "base",
                 fields: [
                   %{"name" => "title", "type" => "text", "required" => true},
                   %{
                     "name" => "published",
                     "type" => "bool",
                     "options" => %{"defaultValue" => false}
                   }
                 ]
               )

      Registry.reload!()

      # The Studio / /api/collections read from the registry — a collection
      # created from a migration must show up there (issue: raw tables don't).
      assert {:ok, coll} = Registry.get(name)
      assert coll.type == "base"
      assert Enum.map(coll.fields, & &1.name) |> Enum.sort() == ["published", "title"]

      assert coll.fields |> Enum.find(&(&1.name == "published")) |> Map.get(:options) ==
               %{"defaultValue" => false}

      # The physical table has the LazyPock shape (uuid id, created_at)
      assert {:ok, %{rows: [row]}} =
               Repo.query(
                 "SELECT data_type FROM information_schema.columns WHERE table_name = $1 AND column_name = 'id'",
                 [name]
               )

      assert row == ["uuid"]
    end

    test "create_collection/2 is idempotent-safe (fails when it exists)" do
      name = "mig_dup_#{:erlang.unique_integer([:positive])}"
      assert {:ok, _} = Migrations.create_collection(name, type: "base", fields: [])
      assert {:error, _} = Migrations.create_collection(name, type: "base", fields: [])
    end

    test "add_field/2 and drop_field/2 keep metadata + table in sync" do
      name = "mig_fields_#{:erlang.unique_integer([:positive])}"
      assert {:ok, _} = Migrations.create_collection(name, type: "base", fields: [])

      assert :ok =
               Migrations.add_field(name, %{
                 "name" => "body",
                 "type" => "text",
                 "required" => false
               })

      Registry.reload!()
      {:ok, coll} = Registry.get(name)
      assert Enum.any?(coll.fields, &(&1.name == "body"))

      assert :ok = Migrations.drop_field(name, "body")
      Registry.reload!()
      {:ok, coll} = Registry.get(name)
      refute Enum.any?(coll.fields, &(&1.name == "body"))
    end

    test "update_collection/2 and drop_collection/1 round-trip" do
      name = "mig_upd_#{:erlang.unique_integer([:positive])}"
      assert {:ok, _} = Migrations.create_collection(name, type: "base", fields: [])
      assert {:ok, _} = Migrations.update_collection(name, rules: %{"listRule" => ""})

      Registry.reload!()
      {:ok, coll} = Registry.get(name)
      assert coll.rules == %{"listRule" => ""}

      assert :ok = Migrations.drop_collection(name)
      Registry.reload!()
      assert Registry.get(name) == {:error, :not_found}
    end
  end
end
