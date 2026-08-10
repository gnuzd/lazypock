defmodule Lazypock.Schema.DDLIndexesTest do
  use Lazypock.DataCase, async: false

  alias Lazypock.Schema.DDL
  alias Lazypock.Repo

  import Ecto.Query

  @collection "idx_test_items"

  setup do
    on_exit(fn ->
      exists =
        Repo.exists?(from(c in Lazypock.Collections.Collection, where: c.name == ^@collection))

      if exists, do: DDL.drop_collection(@collection)
    end)

    :ok
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

  describe "unique fields" do
    test "add_field creates a UNIQUE index when unique: true" do
      {:ok, _coll} =
        DDL.create_collection(@collection,
          type: "base",
          fields: [%{"name" => "title", "type" => "text"}]
        )

      :ok = DDL.add_field(@collection, %{"name" => "sku", "type" => "text", "unique" => true})

      idxs = indexes_for(@collection)
      assert "idx_test_items_sku_unq" in idxs
      assert "idx_test_items_sku_idx" not in idxs

      # Enforce uniqueness at the DB level
      Ecto.Adapters.SQL.query!(
        Repo,
        "INSERT INTO idx_test_items (title, sku) VALUES ('a', 'S1')",
        []
      )

      assert_raise Postgrex.Error, fn ->
        Ecto.Adapters.SQL.query!(
          Repo,
          "INSERT INTO idx_test_items (title, sku) VALUES ('b', 'S1')",
          []
        )
      end
    end

    test "update_collection new field honors unique" do
      {:ok, _coll} =
        DDL.create_collection(@collection,
          type: "base",
          fields: [%{"name" => "title", "type" => "text"}]
        )

      {:ok, _} =
        DDL.update_collection(@collection,
          fields: [
            %{"name" => "title", "type" => "text"},
            %{"name" => "code", "type" => "text", "unique" => true}
          ]
        )

      idxs = indexes_for(@collection)
      assert "idx_test_items_code_unq" in idxs
    end

    test "update_collection toggling unique on an existing field syncs the DB index" do
      {:ok, _coll} =
        DDL.create_collection(@collection,
          type: "base",
          fields: [%{"name" => "title", "type" => "text", "indexed" => true}]
        )

      idxs = indexes_for(@collection)
      assert "idx_test_items_title_idx" in idxs
      assert "idx_test_items_title_unq" not in idxs

      # Now set unique: true (indexed also true) — should replace the plain index
      {:ok, _} =
        DDL.update_collection(@collection,
          fields: [%{"name" => "title", "type" => "text", "indexed" => true, "unique" => true}]
        )

      idxs = indexes_for(@collection)
      assert "idx_test_items_title_unq" in idxs

      # Uniqueness is enforced
      Ecto.Adapters.SQL.query!(Repo, "INSERT INTO idx_test_items (title) VALUES ('dup')", [])

      assert_raise Postgrex.Error, fn ->
        Ecto.Adapters.SQL.query!(Repo, "INSERT INTO idx_test_items (title) VALUES ('dup')", [])
      end

      # Toggle off unique — index should be dropped
      {:ok, _} =
        DDL.update_collection(@collection,
          fields: [%{"name" => "title", "type" => "text", "indexed" => true, "unique" => false}]
        )

      idxs = indexes_for(@collection)
      assert "idx_test_items_title_unq" not in idxs
      assert "idx_test_items_title_idx" in idxs
    end
  end

  describe "custom multi-column indexes" do
    test "create_collection with indexes creates them and persists in options" do
      {:ok, coll} =
        DDL.create_collection(@collection,
          type: "base",
          fields: [
            %{"name" => "title", "type" => "text"},
            %{"name" => "category", "type" => "text"}
          ],
          indexes: ["UNIQUE title, category"]
        )

      idxs = indexes_for(@collection)
      assert Enum.any?(idxs, &String.starts_with?(&1, "_cx_"))

      # Persisted in options
      reloaded = Repo.get!(Lazypock.Collections.Collection, coll.id)
      assert Map.get(reloaded.options || %{}, "indexes") == ["UNIQUE title, category"]
    end

    test "update_collection diff syncs custom indexes" do
      {:ok, coll} =
        DDL.create_collection(@collection,
          type: "base",
          fields: [%{"name" => "title", "type" => "text"}],
          indexes: ["title"]
        )

      old_idx = indexes_for(@collection) |> Enum.find(&String.starts_with?(&1, "_cx_"))
      assert old_idx

      # Remove the old index, add a new one
      {:ok, _} =
        DDL.update_collection(@collection,
          fields: [%{"name" => "title", "type" => "text"}],
          indexes: ["UNIQUE title"]
        )

      idxs = indexes_for(@collection)
      assert old_idx not in idxs
      assert Enum.any?(idxs, &String.starts_with?(&1, "_cx_"))

      # Persisted options updated
      reloaded = Repo.get!(Lazypock.Collections.Collection, coll.id)
      assert Map.get(reloaded.options || %{}, "indexes") == ["UNIQUE title"]
    end

    test "dropping all custom indexes removes the DB index" do
      {:ok, coll} =
        DDL.create_collection(@collection,
          type: "base",
          fields: [%{"name" => "title", "type" => "text"}],
          indexes: ["title"]
        )

      old_idx = indexes_for(@collection) |> Enum.find(&String.starts_with?(&1, "_cx_"))
      assert old_idx

      {:ok, _} =
        DDL.update_collection(@collection,
          fields: [%{"name" => "title", "type" => "text"}],
          indexes: []
        )

      idxs = indexes_for(@collection)
      assert old_idx not in idxs

      reloaded = Repo.get!(Lazypock.Collections.Collection, coll.id)
      assert Map.get(reloaded.options || %{}, "indexes") == []
    end
  end
end
