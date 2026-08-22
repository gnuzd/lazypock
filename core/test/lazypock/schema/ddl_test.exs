defmodule Lazypock.Schema.DDLTest do
  use Lazypock.DataCase, async: false

  alias Lazypock.Schema.DDL
  alias Lazypock.Repo

  import Ecto.Query

  # Unique names per describe block to avoid collisions across tests.
  defp cname(prefix), do: "#{prefix}_#{System.unique_integer([:positive]) |> abs()}"

  defp table_columns(table) do
    Ecto.Adapters.SQL.query!(
      Repo,
      "SELECT column_name, data_type, is_nullable, column_default FROM information_schema.columns WHERE table_name = $1 ORDER BY ordinal_position",
      [table]
    )
    |> Map.fetch!(:rows)
  end

  defp indexes_for(table) do
    Ecto.Adapters.SQL.query!(
      Repo,
      "SELECT indexname FROM pg_indexes WHERE tablename = $1 ORDER BY indexname",
      [table]
    )
    |> Map.fetch!(:rows)
    |> Enum.map(fn [name] -> name end)
  end

  describe "create_collection/2" do
    test "creates a base collection with table + metadata" do
      name = cname("base")

      {:ok, coll} =
        DDL.create_collection(name,
          type: "base",
          fields: [%{"name" => "title", "type" => "text", "required" => true}]
        )

      assert coll.name == name
      assert coll.type == "base"
      assert coll.system == false
      assert coll.managed == true

      # Table exists with PK and timestamps
      cols = table_columns(name) |> Enum.map(fn [c, t, _n, _d] -> {c, t} end)
      assert {"id", "uuid"} in cols
      assert {"title", "text"} in cols
      assert {"created_at", "timestamp with time zone"} in cols
      assert {"updated_at", "timestamp with time zone"} in cols

      # Field metadata persisted
      fields = Repo.all(from(f in Lazypock.Collections.Field, where: f.collection_id == ^coll.id))
      assert length(fields) == 1
      assert hd(fields).name == "title"
      assert hd(fields).required == true
    end

    test "base collections get PocketBase default rules" do
      name = cname("baserules")
      {:ok, coll} = DDL.create_collection(name, type: "base", fields: [])

      assert coll.rules["listRule"] == ""
      assert coll.rules["viewRule"] == ""
      assert coll.rules["createRule"] == "@request.auth.id != ''"
      assert coll.rules["updateRule"] == ""
      assert coll.rules["deleteRule"] == ""
      assert coll.rules["manageRule"] == nil
    end

    test "auth collections get PocketBase auth default rules" do
      name = cname("authrules")
      {:ok, coll} = DDL.create_collection(name, type: "auth", fields: [])

      assert coll.rules["listRule"] == ""
      assert coll.rules["viewRule"] == ""
      assert coll.rules["createRule"] == ""
      assert coll.rules["updateRule"] == "id = @request.auth.id"
      assert coll.rules["deleteRule"] == "id = @request.auth.id"
      assert coll.rules["manageRule"] == nil
    end

    test "rejects invalid collection names" do
      for bad <- ["Uppercase", "has space", "1starts_with_digit", "has-dash", ""] do
        assert {:error, msg} = DDL.create_collection(bad, type: "base", fields: [])
        assert msg =~ "Collection name must start with a letter"
      end
    end

    test "rejects duplicate collection names" do
      name = cname("dup")
      {:ok, _} = DDL.create_collection(name, type: "base", fields: [])
      assert {:error, msg} = DDL.create_collection(name, type: "base", fields: [])
      assert msg =~ "already exists"
    end

    test "rejects duplicate field names" do
      name = cname("dupfield")

      assert {:error, msg} =
               DDL.create_collection(name,
                 type: "base",
                 fields: [
                   %{"name" => "title", "type" => "text"},
                   %{"name" => "title", "type" => "text"}
                 ]
               )

      assert msg =~ "Duplicate field names"
    end

    test "rejects invalid field names" do
      name = cname("badfield")

      assert {:error, msg} =
               DDL.create_collection(name,
                 type: "base",
                 fields: [%{"name" => "Bad Name", "type" => "text"}]
               )

      assert msg =~ "Field names must start with a letter"
    end

    test "allows mixed-case field names — column is lowercased, name kept verbatim" do
      name = cname("mixedcase")

      assert {:ok, coll} =
               DDL.create_collection(name,
                 type: "base",
                 fields: [
                   %{"name" => "tagColor", "type" => "text", "required" => false},
                   %{"name" => "displayName", "type" => "text", "required" => false}
                 ]
               )

      # Metadata name is verbatim (camelCase)
      assert Enum.map(coll.fields, & &1.name) |> Enum.sort() == ["displayName", "tagColor"]

      # DB columns are the lowercase forms
      cols =
        Repo.query!(
          "SELECT column_name FROM information_schema.columns WHERE table_name = $1",
          [name]
        )
        |> Map.get(:rows)
        |> List.flatten()

      assert "tagcolor" in cols
      assert "displayname" in cols
      refute "tagColor" in cols

      # FieldNames bridges: insert via lowercase column, read back via metadata name
      assert {:ok, _} = Lazypock.Schemas.GenericRecord.insert(name, %{"tagcolor" => "red"})

      {:ok, coll} = Lazypock.Collections.Registry.get(name)

      assert [%{"tagColor" => "red"}] =
               name
               |> Lazypock.Schemas.GenericRecord.all()
               |> Enum.map(&Lazypock.Schemas.FieldNames.row_to_api(&1, coll))
    end

    test "rejects invalid field types" do
      name = cname("badtype")

      assert {:error, msg} =
               DDL.create_collection(name,
                 type: "base",
                 fields: [%{"name" => "x", "type" => "nope"}]
               )

      assert msg =~ "Invalid field type"
    end

    test "multi_select and multi_file fields create array columns" do
      name = cname("arrays")

      {:ok, _} =
        DDL.create_collection(name,
          type: "base",
          fields: [
            %{"name" => "tags", "type" => "multi_select"},
            %{"name" => "docs", "type" => "multi_file"}
          ]
        )

      cols = table_columns(name) |> Enum.map(fn [c, t, _n, _d] -> {c, t} end)
      assert {"tags", "ARRAY"} in cols
      assert {"docs", "ARRAY"} in cols
    end

    test "json field creates jsonb column" do
      name = cname("jsonb")

      {:ok, _} =
        DDL.create_collection(name, type: "base", fields: [%{"name" => "meta", "type" => "json"}])

      cols = table_columns(name) |> Enum.map(fn [c, t, _n, _d] -> {c, t} end)
      assert {"meta", "jsonb"} in cols
    end

    test "text default value is applied at the DB level" do
      name = cname("default")

      {:ok, _} =
        DDL.create_collection(name,
          type: "base",
          fields: [%{"name" => "status", "type" => "text", "default" => "draft"}]
        )

      Ecto.Adapters.SQL.query!(Repo, "INSERT INTO #{name} DEFAULT VALUES", [])
      {:ok, %{rows: [[status]]}} = Ecto.Adapters.SQL.query(Repo, "SELECT status FROM #{name}", [])
      assert status == "draft"
    end
  end

  describe "add_field/3" do
    test "adds a column and metadata" do
      name = cname("addf")
      {:ok, _} = DDL.create_collection(name, type: "base", fields: [])

      assert :ok = DDL.add_field(name, %{"name" => "note", "type" => "text"})

      cols = table_columns(name) |> Enum.map(fn [c, _t, _n, _d] -> c end)
      assert "note" in cols

      coll = Repo.get_by(Lazypock.Collections.Collection, name: name)
      fields = Repo.all(from(f in Lazypock.Collections.Field, where: f.collection_id == ^coll.id))
      assert Enum.any?(fields, &(&1.name == "note"))
    end

    test "required fields are NOT NULL" do
      name = cname("addreq")
      {:ok, _} = DDL.create_collection(name, type: "base", fields: [])

      assert :ok = DDL.add_field(name, %{"name" => "req", "type" => "text", "required" => true})

      assert_raise Postgrex.Error, fn ->
        Ecto.Adapters.SQL.query!(Repo, "INSERT INTO #{name} DEFAULT VALUES", [])
      end
    end

    test "indexed and unique flags create DB indexes" do
      name = cname("addidx")
      {:ok, _} = DDL.create_collection(name, type: "base", fields: [])

      :ok = DDL.add_field(name, %{"name" => "a", "type" => "text", "indexed" => true})
      :ok = DDL.add_field(name, %{"name" => "b", "type" => "text", "unique" => true})

      idxs = indexes_for(name)
      assert "#{name}_a_idx" in idxs
      assert "#{name}_b_unq" in idxs
    end

    test "rejects invalid names and types" do
      name = cname("addbad")
      {:ok, _} = DDL.create_collection(name, type: "base", fields: [])

      assert {:error, msg} = DDL.add_field(name, %{"name" => "Bad Name", "type" => "text"})
      assert msg =~ "must start with a letter"

      assert {:error, msg} = DDL.add_field(name, %{"name" => "ok", "type" => "bogus"})
      assert msg =~ "Invalid field type"
    end

    test "relation field resolves collectionId to a collection name" do
      target = cname("reldest")
      {:ok, _} = DDL.create_collection(target, type: "base", fields: [])
      name = cname("relsrc")
      {:ok, src} = DDL.create_collection(name, type: "base", fields: [])

      :ok =
        DDL.add_field(name, %{
          "name" => "parent",
          "type" => "relation",
          "collectionId" => src.id,
          "options" => %{}
        })

      coll = Repo.get_by(Lazypock.Collections.Collection, name: name)
      fields = Repo.all(from(f in Lazypock.Collections.Field, where: f.collection_id == ^coll.id))
      rel = Enum.find(fields, &(&1.name == "parent"))
      # collectionId refers to src (relsrc), so the normalized option is src's name
      assert rel.options["collection"] == name
    end
  end

  describe "drop_field/3" do
    test "removes column and metadata" do
      name = cname("dropf")

      {:ok, _} =
        DDL.create_collection(name,
          type: "base",
          fields: [%{"name" => "keep", "type" => "text"}, %{"name" => "gone", "type" => "text"}]
        )

      assert :ok = DDL.drop_field(name, "gone")

      cols = table_columns(name) |> Enum.map(fn [c, _t, _n, _d] -> c end)
      refute "gone" in cols
      assert "keep" in cols

      coll = Repo.get_by(Lazypock.Collections.Collection, name: name)
      fields = Repo.all(from(f in Lazypock.Collections.Field, where: f.collection_id == ^coll.id))
      refute Enum.any?(fields, &(&1.name == "gone"))
    end
  end

  describe "update_collection/2" do
    test "renames the table and metadata" do
      old = cname("ren_old")
      new = cname("ren_new")

      {:ok, _} =
        DDL.create_collection(old, type: "base", fields: [%{"name" => "title", "type" => "text"}])

      {:ok, coll} = DDL.update_collection(old, name: new)
      assert coll.name == new

      # New table has the columns; the old table is gone (no rows in catalog)
      cols = table_columns(new) |> Enum.map(fn [c, _t, _n, _d] -> c end)
      assert "title" in cols
      assert table_columns(old) == []
    end

    test "system collections cannot be renamed" do
      coll = Repo.get_by(Lazypock.Collections.Collection, name: "_superusers")

      if coll do
        assert {:error, msg} = DDL.update_collection("_superusers", name: "not_allowed")
        assert msg =~ "Cannot rename system collection"
      end
    end

    test "changes collection type" do
      name = cname("typ")
      {:ok, _} = DDL.create_collection(name, type: "base", fields: [])

      {:ok, coll} = DDL.update_collection(name, type: "auth")
      assert coll.type == "auth"
    end

    test "updates rules metadata" do
      name = cname("rulesmeta")
      {:ok, _} = DDL.create_collection(name, type: "base", fields: [])

      {:ok, coll} =
        DDL.update_collection(name, rules: %{"listRule" => "active = true"})

      assert coll.rules["listRule"] == "active = true"
    end

    test "adds and removes fields in one update" do
      name = cname("fieldsync")

      {:ok, _} =
        DDL.create_collection(name,
          type: "base",
          fields: [%{"name" => "keep", "type" => "text"}, %{"name" => "dropme", "type" => "text"}]
        )

      {:ok, _} =
        DDL.update_collection(name,
          fields: [
            %{"name" => "keep", "type" => "text"},
            %{"name" => "fresh", "type" => "number"}
          ]
        )

      cols = table_columns(name) |> Enum.map(fn [c, _t, _n, _d] -> c end)
      assert "keep" in cols
      assert "fresh" in cols
      refute "dropme" in cols
    end

    test "persists sort_order from array position" do
      name = cname("sortord")

      {:ok, _} =
        DDL.create_collection(name,
          type: "base",
          fields: [%{"name" => "a", "type" => "text"}, %{"name" => "b", "type" => "text"}]
        )

      {:ok, _} =
        DDL.update_collection(name,
          fields: [%{"name" => "b", "type" => "text"}, %{"name" => "a", "type" => "text"}]
        )

      coll = Repo.get_by(Lazypock.Collections.Collection, name: name)

      fields =
        Repo.all(
          from(f in Lazypock.Collections.Field,
            where: f.collection_id == ^coll.id,
            order_by: f.sort_order
          )
        )

      assert Enum.map(fields, & &1.name) == ["b", "a"]
    end
  end

  describe "drop_collection/1" do
    test "drops a managed collection table and metadata" do
      name = cname("dropcoll")
      {:ok, _} = DDL.create_collection(name, type: "base", fields: [])
      assert :ok = DDL.drop_collection(name)
      assert Repo.get_by(Lazypock.Collections.Collection, name: name) == nil
    end

    test "system collections are protected" do
      assert {:error, msg} = DDL.drop_collection("_superusers")
      assert msg =~ "Cannot delete system collection"
    end

    test "unknown collection returns error" do
      assert {:error, :not_found} = DDL.drop_collection("does_not_exist_xyz")
    end
  end
end
