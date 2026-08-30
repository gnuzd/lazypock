defmodule Lazypock.Schema.ViewsTest do
  use Lazypock.DataCase, async: false

  alias Lazypock.Schema.DDL
  alias Lazypock.Schema.Views
  alias Lazypock.Repo

  defp cname(prefix), do: "#{prefix}_#{System.unique_integer([:positive]) |> abs()}"

  # Creates a small base collection to serve as the view query source.
  defp create_source_collection(name) do
    {:ok, _coll} =
      DDL.create_collection(name,
        type: "base",
        fields: [
          %{"name" => "title", "type" => "text", "required" => false},
          %{"name" => "count", "type" => "number", "required" => false}
        ]
      )

    :ok
  end

  defp query_rows(sql, params \\ []) do
    Ecto.Adapters.SQL.query!(Repo, sql, params).rows
  end

  describe "build_fields/1" do
    test "derives fields from a valid query (id text, count → number)" do
      src = cname("src")
      create_source_collection(src)

      {:ok, fields} =
        Views.build_fields("SELECT id, title, count(id) AS total FROM #{src} GROUP BY id")

      assert Enum.map(fields, & &1["name"]) == ["id", "title", "total"]

      assert %{"name" => "id", "type" => "text", "required" => true, "system" => true} =
               Enum.find(fields, &(&1["name"] == "id"))

      assert %{"name" => "title", "type" => "text"} = Enum.find(fields, &(&1["name"] == "title"))

      assert %{"name" => "total", "type" => "number"} =
               Enum.find(fields, &(&1["name"] == "total"))
    end

    test "rejects queries without an id column" do
      src = cname("src")
      create_source_collection(src)

      assert {:error, message} =
               Views.build_fields("SELECT title FROM #{src}")

      assert message =~ "missing required id column"
    end

    test "rejects wildcard columns" do
      src = cname("src")
      create_source_collection(src)

      assert {:error, message} = Views.build_fields("SELECT * FROM #{src}")
      assert message =~ "wildcard columns (*) are not supported"

      assert {:error, message} = Views.build_fields("SELECT t.* FROM #{src} AS t")
      assert message =~ "wildcard columns (*) are not supported"

      # count(*) is fine — the star is inside parentheses
      assert {:ok, _fields} =
               Views.build_fields("SELECT id, count(*) AS total FROM #{src} GROUP BY id")
    end

    test "rejects multiple statements" do
      assert {:error, message} = Views.build_fields("SELECT 1; SELECT 2")
      assert message =~ "multiple statements are not supported"

      # Semicolon inside a string literal is fine
      src = cname("src")
      create_source_collection(src)

      assert {:ok, _fields} =
               Views.build_fields("SELECT id, title FROM #{src} WHERE title = 'a;b'")
    end

    test "rejects invalid SQL / missing tables with a Postgres error" do
      assert {:error, message} = Views.build_fields("SELECT id FROM missing_table_xyz")
      assert message =~ "missing_table_xyz"
    end
  end

  describe "create_collection with type view" do
    test "creates a physical view with derived fields" do
      src = cname("src")
      create_source_collection(src)
      name = cname("view")

      {:ok, coll} =
        DDL.create_collection(name,
          type: "view",
          options: %{"view_query" => "SELECT id, title FROM #{src}"}
        )

      assert coll.type == "view"
      assert [%{name: "id"}, %{name: "title"}] = Enum.map(coll.fields, & &1)
      assert coll.options["view_query"] =~ src

      # The view is queryable
      assert query_rows("SELECT count(*) FROM #{name}") == [[0]]

      # views live in the view catalog, not the table catalog
      [[table]] =
        Ecto.Adapters.SQL.query!(
          Repo,
          "SELECT table_name FROM information_schema.views WHERE table_name = $1",
          [name]
        ).rows

      assert table == name
    end

    test "casts a non-text id column to TEXT" do
      src = cname("src")
      create_source_collection(src)
      name = cname("view")

      # The source id is a UUID column; the view must expose it as TEXT so
      # record lookups (WHERE id = $1 with a string) work.
      {:ok, _coll} =
        DDL.create_collection(name,
          type: "view",
          options: %{"view_query" => "SELECT id FROM #{src}"}
        )

      [[data_type]] =
        Ecto.Adapters.SQL.query!(
          Repo,
          "SELECT data_type FROM information_schema.columns WHERE table_name = $1 AND column_name = 'id'",
          [name]
        ).rows

      assert data_type == "text"
    end

    test "rolls back when the view query is invalid" do
      src = cname("src")
      create_source_collection(src)
      name = cname("view")

      assert {:error, _reason} =
               DDL.create_collection(name,
                 type: "view",
                 options: %{"view_query" => "SELECT title FROM #{src}"}
               )

      # No metadata and no physical view should exist
      assert Repo.get_by(Lazypock.Collections.Collection, name: name) == nil

      assert query_rows("SELECT count(*) FROM information_schema.views WHERE table_name = $1", [
               name
             ]) ==
               [[0]]
    end

    test "requires a view query" do
      assert {:error, "view query is required"} =
               DDL.create_collection(cname("view"), type: "view", options: %{})
    end

    test "record mutations on a view are rejected by the DDL layer is not needed (controller-level)" do
      # Sanity: a view collection can be created with rules and listed.
      src = cname("src")
      create_source_collection(src)
      name = cname("view")

      {:ok, coll} =
        DDL.create_collection(name,
          type: "view",
          options: %{"view_query" => "SELECT id, title FROM #{src}"},
          rules: %{"listRule" => "", "viewRule" => ""}
        )

      assert coll.rules["listRule"] == ""
    end
  end

  describe "update_collection on a view" do
    test "rebuilds fields + view when the query changes" do
      src = cname("src")
      create_source_collection(src)
      name = cname("view")

      {:ok, _coll} =
        DDL.create_collection(name,
          type: "view",
          options: %{"view_query" => "SELECT id, title FROM #{src}"}
        )

      {:ok, coll} =
        DDL.update_collection(name,
          options: %{"view_query" => "SELECT id, title, count FROM #{src}"}
        )

      assert Enum.map(coll.fields, & &1.name) == ["id", "title", "count"]
      assert coll.options["view_query"] =~ "title, count"
    end

    test "rejects field payloads (fields are derived from the query)" do
      src = cname("src")
      create_source_collection(src)
      name = cname("view")

      {:ok, _coll} =
        DDL.create_collection(name,
          type: "view",
          options: %{"view_query" => "SELECT id, title FROM #{src}"}
        )

      assert {:error, message} =
               DDL.update_collection(name, fields: [%{"name" => "extra", "type" => "text"}])

      assert message =~ "auto-generated"
    end

    test "renames the physical view" do
      src = cname("src")
      create_source_collection(src)
      name = cname("view")
      new_name = cname("renamed")

      {:ok, _coll} =
        DDL.create_collection(name,
          type: "view",
          options: %{"view_query" => "SELECT id, title FROM #{src}"}
        )

      {:ok, coll} = DDL.update_collection(name, name: new_name)

      assert coll.name == new_name

      assert query_rows("SELECT count(*) FROM information_schema.views WHERE table_name = $1", [
               name
             ]) ==
               [[0]]

      assert query_rows("SELECT count(*) FROM information_schema.views WHERE table_name = $1", [
               new_name
             ]) ==
               [[1]]
    end
  end

  describe "drop_collection on a view" do
    test "drops the view and metadata" do
      src = cname("src")
      create_source_collection(src)
      name = cname("view")

      {:ok, _coll} =
        DDL.create_collection(name,
          type: "view",
          options: %{"view_query" => "SELECT id, title FROM #{src}"}
        )

      assert :ok = DDL.drop_collection(name)
      assert Repo.get_by(Lazypock.Collections.Collection, name: name) == nil

      assert query_rows("SELECT count(*) FROM information_schema.views WHERE table_name = $1", [
               name
             ]) ==
               [[0]]
    end
  end

  describe "dry_run/2" do
    test "returns fields + sample records" do
      src = cname("src")
      create_source_collection(src)

      {:ok, record} =
        Lazypock.Schemas.GenericRecord.insert(src, %{"title" => "hello", "count" => 3})

      {:ok, %{fields: fields, sample: sample}} =
        Views.dry_run("SELECT id, title, count FROM #{src}")

      assert Enum.map(fields, & &1["name"]) == ["id", "title", "count"]
      assert length(sample) == 1
      assert sample |> hd() |> Map.get("title") == "hello"
      assert sample |> hd() |> Map.get("id") == record["id"]
    end

    test "rejects queries with non-unique sample ids" do
      src = cname("src")
      create_source_collection(src)
      Lazypock.Schemas.GenericRecord.insert(src, %{"title" => "a", "count" => 1})
      Lazypock.Schemas.GenericRecord.insert(src, %{"title" => "b", "count" => 2})

      assert {:error, message} = Views.dry_run("SELECT 'same-id' AS id, title FROM #{src}")
      assert message =~ "non-unique ids"
    end
  end

  describe "sync_dependent_views/1" do
    test "survives a view whose query became invalid (drops are logged, not fatal)" do
      src = cname("src")
      create_source_collection(src)
      name = cname("view")

      {:ok, _coll} =
        DDL.create_collection(name,
          type: "view",
          options: %{"view_query" => "SELECT id, title FROM #{src}"}
        )

      # Dropping the source table cascades to the dependent view. Sync must
      # not raise — it logs the failure and leaves the metadata intact so the
      # query can be fixed from the UI (PocketBase behavior).
      DDL.drop_collection(src)
      assert :ok = DDL.sync_dependent_views(src)

      assert Repo.get_by(Lazypock.Collections.Collection, name: name) != nil
    end

    test "re-introspects views after a source change when the query still resolves" do
      src = cname("src")
      create_source_collection(src)
      name = cname("view")

      {:ok, _coll} =
        DDL.create_collection(name,
          type: "view",
          options: %{"view_query" => "SELECT id, title FROM #{src}"}
        )

      # A source rename keeps the view queryable (Postgres rewrites the view
      # definition); sync drops and recreates it from the stored query.
      {:ok, _} = DDL.update_collection(src, name: src <> "_renamed")
      assert :ok = DDL.sync_dependent_views(name)

      # The rebuilt view still resolves against the renamed source.
      assert query_rows("SELECT count(*) FROM #{name}") == [[0]]
    end
  end
end
