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

    test "create_collection/2 rejects a relation to an unknown collection" do
      name = "mig_relbad_#{:erlang.unique_integer([:positive])}"
      missing = "mig_nope_#{:erlang.unique_integer([:positive])}"

      assert {:error, msg} =
               Migrations.create_collection(name,
                 type: "base",
                 fields: [
                   %{
                     "name" => "owner",
                     "type" => "relation",
                     "options" => %{"collection" => missing, "maxSelect" => 1}
                   }
                 ]
               )

      assert msg =~ "owner"
      assert msg =~ missing

      # Fail-fast: nothing was persisted (no collection, no table).
      assert {:error, _} = Registry.get(name)
      assert {:ok, %{rows: [[nil]]}} = Repo.query("SELECT to_regclass('public.\"#{name}\"')")
    end

    test "create_collection/2 accepts a relation to an existing collection" do
      target = "mig_reltarget_#{:erlang.unique_integer([:positive])}"
      assert {:ok, _} = Migrations.create_collection(target, type: "base", fields: [])

      name = "mig_relok_#{:erlang.unique_integer([:positive])}"

      assert {:ok, _collection} =
               Migrations.create_collection(name,
                 type: "base",
                 fields: [
                   %{
                     "name" => "owner",
                     "type" => "relation",
                     "options" => %{"collection" => target, "maxSelect" => 1}
                   }
                 ]
               )

      Registry.reload!()
      {:ok, coll} = Registry.get(name)
      owner = Enum.find(coll.fields, &(&1.name == "owner"))
      assert owner.options["collection"] == target
    end

    test "add_field/2 rejects a relation to an unknown collection" do
      name = "mig_reladdbad_#{:erlang.unique_integer([:positive])}"
      assert {:ok, _} = Migrations.create_collection(name, type: "base", fields: [])

      missing = "mig_nope2_#{:erlang.unique_integer([:positive])}"

      assert {:error, msg} =
               Migrations.add_field(name, %{
                 "name" => "parent",
                 "type" => "relation",
                 "options" => %{"collection" => missing, "maxSelect" => 1}
               })

      assert msg =~ "parent"
      assert msg =~ missing
    end

    test "add_field/2 accepts a relation to an existing collection" do
      target = "mig_reladdtarget_#{:erlang.unique_integer([:positive])}"
      assert {:ok, _} = Migrations.create_collection(target, type: "base", fields: [])

      name = "mig_reladdok_#{:erlang.unique_integer([:positive])}"
      assert {:ok, _} = Migrations.create_collection(name, type: "base", fields: [])

      assert :ok =
               Migrations.add_field(name, %{
                 "name" => "parent",
                 "type" => "relation",
                 "options" => %{"collection" => target, "maxSelect" => 1}
               })

      Registry.reload!()
      {:ok, coll} = Registry.get(name)
      parent = Enum.find(coll.fields, &(&1.name == "parent"))
      assert parent.options["collection"] == target
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

    test "foreign-key columns become relation fields pointing at the referenced table" do
      target = "mig_authors_#{:erlang.unique_integer([:positive])}"
      referencing = "mig_books_#{:erlang.unique_integer([:positive])}"

      Ecto.Adapters.SQL.query!(
        Repo,
        """
        CREATE TABLE #{target} (
          id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
          name TEXT NOT NULL,
          created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
          updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        """,
        []
      )

      Ecto.Adapters.SQL.query!(
        Repo,
        """
        CREATE TABLE #{referencing} (
          id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
          title TEXT NOT NULL,
          author_id UUID REFERENCES #{target}(id),
          created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
          updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        """,
        []
      )

      on_exit(fn ->
        Ecto.Adapters.SQL.query!(Repo, "DROP TABLE IF EXISTS #{referencing}", [])
        Ecto.Adapters.SQL.query!(Repo, "DROP TABLE IF EXISTS #{target}", [])
      end)

      assert :ok = Migrations.register_unregistered_tables()
      Registry.reload!()

      {:ok, coll} = Registry.get(referencing)
      author_field = Enum.find(coll.fields, &(&1.name == "author_id"))
      assert author_field.type == "relation"
      assert author_field.options == %{"collection" => target}

      # The referenced table is registered too, so the relation target resolves
      # through the registry (the Studio dropdown + expand depend on this).
      assert {:ok, _target_coll} = Registry.get(target)

      # And the field is exposed to the API with a resolved collectionId.
      {:ok, %{rows: field_rows}} =
        Repo.query(
          "SELECT name, type, options FROM _fields WHERE collection_id = $1",
          [Ecto.UUID.dump!(coll.id)]
        )

      assert ["author_id", "relation", %{"collection" => ^target}] =
               Enum.find(field_rows, fn [name, _type, _opts] -> name == "author_id" end)

      # End-to-end: a record in the referencing table stores the target id, and
      # the API's expand path resolves it to the related record (this is what
      # the Studio's record browser renders as the dropdown value).
      {:ok, author} = Lazypock.Schemas.GenericRecord.insert(target, %{"name" => "Ada"})

      # Seed the referencing row like a raw migration's own data would (the
      # FK column here is `uuid`, so the id goes in as a 16-byte binary).
      Ecto.Adapters.SQL.query!(
        Repo,
        """
        INSERT INTO #{referencing} (title, author_id, created_at, updated_at)
        VALUES ('Notes', $1, now(), now()) RETURNING id
        """,
        [Ecto.UUID.dump!(author["id"])]
      )

      book =
        Repo.query!("SELECT * FROM #{referencing}", [])
        |> then(fn %{rows: [row], columns: cols} ->
          cols
          |> Enum.zip(row)
          |> Map.new()
          |> Map.new(fn {k, v} ->
            {k, if(is_binary(v) and byte_size(v) == 16, do: Ecto.UUID.cast!(v), else: v)}
          end)
        end)

      [expanded] = LazypockWeb.DynamicView.expand_records([book], "author_id", referencing)
      assert expanded["expand"]["author_id"]["name"] == "Ada"
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
