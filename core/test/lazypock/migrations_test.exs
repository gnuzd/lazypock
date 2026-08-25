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

  describe "raw create-table migrations are auto-registered" do
    test "an existing public table becomes a collection visible to the registry" do
      name = "mig_raw_#{:erlang.unique_integer([:positive])}"

      # Simulate a raw `create table` migration (no _collections/_fields rows).
      Ecto.Adapters.SQL.query!(
        Repo,
        """
        CREATE TABLE #{name} (
          id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
          title TEXT NOT NULL,
          published BOOLEAN NOT NULL DEFAULT false,
          views BIGINT,
          created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
          updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        """,
        []
      )

      on_exit(fn -> Ecto.Adapters.SQL.query!(Repo, "DROP TABLE IF EXISTS #{name}", []) end)

      assert :ok = Migrations.register_unregistered_tables()
      Registry.reload!()

      assert {:ok, coll} = Registry.get(name)
      assert coll.type == "base"

      names = Enum.map(coll.fields, & &1.name) |> Enum.sort()
      assert names == ["published", "title", "views"]

      types = Map.new(coll.fields, &{&1.name, &1.type})
      assert types["title"] == "text"
      assert types["published"] == "bool"
      assert types["views"] == "number"
      assert Enum.find(coll.fields, &(&1.name == "title")).required
      refute Enum.find(coll.fields, &(&1.name == "published")).required
    end

    test "registering is idempotent" do
      name = "mig_raw2_#{:erlang.unique_integer([:positive])}"

      Ecto.Adapters.SQL.query!(
        Repo,
        """
        CREATE TABLE #{name} (
          id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
          title TEXT,
          created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
          updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        """,
        []
      )

      on_exit(fn -> Ecto.Adapters.SQL.query!(Repo, "DROP TABLE IF EXISTS #{name}", []) end)

      assert :ok = Migrations.register_unregistered_tables()
      assert :ok = Migrations.register_unregistered_tables()

      count =
        Repo.query!("SELECT count(*) FROM _collections WHERE name = $1", [name]).rows
        |> List.flatten()
        |> hd()

      assert count == 1
    end

    test "Ecto timestamps() shape is reconciled (inserted_at -> created_at, updated_at added)" do
      name = "mig_ecto_#{:erlang.unique_integer([:positive])}"

      Ecto.Adapters.SQL.query!(
        Repo,
        """
        CREATE TABLE #{name} (
          id BIGSERIAL PRIMARY KEY,
          title TEXT NOT NULL,
          inserted_at timestamp NOT NULL,
          updated_at timestamp NOT NULL
        )
        """,
        []
      )

      on_exit(fn -> Ecto.Adapters.SQL.query!(Repo, "DROP TABLE IF EXISTS #{name}", []) end)

      assert :ok = Migrations.register_unregistered_tables()
      Registry.reload!()
      assert {:ok, _} = Registry.get(name)

      # inserted_at was renamed to created_at; updated_at already existed
      {:ok, %{rows: rows}} =
        Repo.query(
          "SELECT column_name FROM information_schema.columns WHERE table_name = $1 ORDER BY ordinal_position",
          [name]
        )

      columns = List.flatten(rows)
      assert "created_at" in columns
      refute "inserted_at" in columns
      assert "updated_at" in columns

      # And CRUD works end-to-end on the registered raw table
      assert {:ok, rec} = Lazypock.Schemas.GenericRecord.insert(name, %{"title" => "hello"})
      assert rec["title"] == "hello"

      assert %{"title" => "updated"} =
               Lazypock.Schemas.GenericRecord.update(
                 name,
                 rec["id"],
                 %{"title" => "updated"}
               )

      assert :ok = Lazypock.Schemas.GenericRecord.delete(name, rec["id"])
    end

    test "internal _ tables and schema_migrations are never registered" do
      name = "_mig_internal_#{:erlang.unique_integer([:positive])}"

      Ecto.Adapters.SQL.query!(
        Repo,
        "CREATE TABLE #{name} (id UUID PRIMARY KEY DEFAULT gen_random_uuid())",
        []
      )

      on_exit(fn -> Ecto.Adapters.SQL.query!(Repo, "DROP TABLE IF EXISTS #{name}", []) end)

      assert :ok = Migrations.register_unregistered_tables()

      count =
        Repo.query!("SELECT count(*) FROM _collections WHERE name = $1", [name]).rows
        |> List.flatten()
        |> hd()

      assert count == 0
    end
  end
end
