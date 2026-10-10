defmodule Lazypock.Schemas.GenericRecordTest do
  use Lazypock.DataCase, async: false

  alias Lazypock.Schema.DDL
  alias Lazypock.Schemas.GenericRecord
  alias Lazypock.Collections.Registry

  defp cname(prefix), do: "#{prefix}_#{System.unique_integer([:positive]) |> abs()}"

  # Collection with an arbitrary autodate field in each trigger configuration
  # plus a plain text field. `last_seen_at` = create/update, `published_at` =
  # create-only, `touched_at` = update-only.
  defp create_autodate_collection do
    name = cname("auto")

    {:ok, _} =
      DDL.create_collection(name,
        type: "base",
        fields: [
          %{"name" => "title", "type" => "text"},
          %{
            "name" => "last_seen_at",
            "type" => "autodate",
            "options" => %{"onCreate" => true, "onUpdate" => true}
          },
          %{"name" => "published_at", "type" => "autodate", "options" => %{"onCreate" => true}},
          %{"name" => "touched_at", "type" => "autodate", "options" => %{"onUpdate" => true}}
        ]
      )

    Registry.reload!()
    name
  end

  defp recent?(iso, within_seconds \\ 5) do
    {:ok, dt, _} = DateTime.from_iso8601(iso)
    diff = DateTime.diff(DateTime.utc_now(), dt, :second)
    diff >= 0 and diff <= within_seconds
  end

  describe "insert/2 — autodate onCreate" do
    setup do
      %{name: create_autodate_collection()}
    end

    test "stamps create/update and create-only autodate fields, not update-only", %{name: name} do
      {:ok, record} = GenericRecord.insert(name, %{"title" => "hello"})

      assert recent?(record["last_seen_at"])
      assert recent?(record["published_at"])
      assert record["touched_at"] == nil
    end

    test "user-supplied values for onCreate autodate fields are overwritten", %{name: name} do
      {:ok, record} =
        GenericRecord.insert(name, %{"title" => "hello", "last_seen_at" => "2020-01-01T00:00:00Z"})

      # Autodate semantics: the field is stamped by the engine, not the caller.
      refute record["last_seen_at"] == "2020-01-01T00:00:00Z"
      assert recent?(record["last_seen_at"])
    end

    test "non-autodate values pass through untouched", %{name: name} do
      {:ok, record} = GenericRecord.insert(name, %{"title" => "kept"})
      assert record["title"] == "kept"
    end
  end

  describe "update/3 — autodate onUpdate" do
    setup do
      %{name: create_autodate_collection()}
    end

    test "stamps create/update and update-only fields, leaves create-only alone", %{name: name} do
      {:ok, record} = GenericRecord.insert(name, %{"title" => "v1"})
      # Wait so the timestamps differ from the insert stamps.
      Process.sleep(1100)

      original = record["published_at"]

      {:ok, updated} = GenericRecord.update(name, record["id"], %{"title" => "v2"})

      assert recent?(updated["last_seen_at"])
      assert recent?(updated["touched_at"])
      # create-only autodate is NOT bumped on update.
      assert updated["published_at"] == original
      # updated_at system column is always bumped.
      assert recent?(updated["updated_at"])
      assert updated["title"] == "v2"
    end

    test "user-supplied values for onUpdate autodate fields are overwritten", %{name: name} do
      {:ok, record} = GenericRecord.insert(name, %{"title" => "v1"})
      Process.sleep(1100)

      {:ok, updated} =
        GenericRecord.update(name, record["id"], %{
          "title" => "v2",
          "last_seen_at" => "2019-05-05T00:00:00Z"
        })

      refute updated["last_seen_at"] == "2019-05-05T00:00:00Z"
      assert recent?(updated["last_seen_at"])
    end

    test "update ignores caller-supplied updated_at (always bumped)", %{name: name} do
      {:ok, record} = GenericRecord.insert(name, %{"title" => "v1"})
      Process.sleep(1100)

      {:ok, updated} =
        GenericRecord.update(name, record["id"], %{
          "title" => "v2",
          "updated_at" => "2018-01-01T00:00:00Z"
        })

      assert recent?(updated["updated_at"])
    end
  end

  describe "insert/update on a collection with user-defined created_at/updated_at autodate fields" do
    test "CRUD works end to end" do
      name = cname("dedupe")

      {:ok, _} =
        DDL.create_collection(name,
          type: "base",
          fields: [
            %{"name" => "title", "type" => "text"},
            %{"name" => "created_at", "type" => "autodate", "options" => %{"onCreate" => true}},
            %{
              "name" => "updated_at",
              "type" => "autodate",
              "options" => %{"onCreate" => true, "onUpdate" => true}
            }
          ]
        )

      Registry.reload!()

      {:ok, record} = GenericRecord.insert(name, %{"title" => "hi"})
      assert recent?(record["created_at"])
      assert recent?(record["updated_at"])

      Process.sleep(1100)
      {:ok, updated} = GenericRecord.update(name, record["id"], %{"title" => "ho"})
      assert recent?(updated["updated_at"])
      assert updated["title"] == "ho"
    end
  end

  describe "update/3 — error reporting and parameter pairing" do
    test "returns {:error, :not_found} for an unknown id" do
      name = create_autodate_collection()
      missing = Ecto.UUID.generate()

      assert {:error, :not_found} = GenericRecord.update(name, missing, %{"title" => "x"})
    end

    test "still bumps updated_at when there is nothing else to write" do
      name = create_autodate_collection()
      {:ok, record} = GenericRecord.insert(name, %{"title" => "keep"})
      Process.sleep(1100)

      assert {:ok, updated} = GenericRecord.update(name, record["id"], %{})
      assert updated["title"] == "keep"
      assert recent?(updated["updated_at"])
    end

    test "never binds updated_at twice, even when the collection declares it as a field" do
      name = cname("dup")

      {:ok, _} =
        DDL.create_collection(name,
          type: "base",
          fields: [
            %{"name" => "title", "type" => "text"},
            %{
              "name" => "updated_at",
              "type" => "autodate",
              "options" => %{"onCreate" => true, "onUpdate" => true}
            }
          ]
        )

      Registry.reload!()

      {:ok, record} = GenericRecord.insert(name, %{"title" => "v1"})
      Process.sleep(1100)

      assert {:ok, updated} = GenericRecord.update(name, record["id"], %{"title" => "v2"})
      assert updated["title"] == "v2"
      assert recent?(updated["updated_at"])
    end

    test "pairs every column with its own value across a multi-field update" do
      name = cname("multi")

      {:ok, _} =
        DDL.create_collection(name,
          type: "base",
          fields: [
            %{"name" => "alpha", "type" => "text"},
            %{"name" => "beta", "type" => "text"},
            %{"name" => "gamma", "type" => "text"},
            %{"name" => "count", "type" => "number"},
            %{"name" => "flag", "type" => "bool"}
          ]
        )

      Registry.reload!()

      {:ok, record} =
        GenericRecord.insert(name, %{
          "alpha" => "a1",
          "beta" => "b1",
          "gamma" => "g1",
          "count" => 1,
          "flag" => false
        })

      {:ok, updated} =
        GenericRecord.update(name, record["id"], %{
          "alpha" => "a2",
          "beta" => "b2",
          "gamma" => "g2",
          "count" => 42,
          "flag" => true
        })

      assert updated["alpha"] == "a2"
      assert updated["beta"] == "b2"
      assert updated["gamma"] == "g2"
      assert updated["count"] == 42
      assert updated["flag"] == true
    end

    test "pairs every column with its own value across a wide (>32 column) update" do
      # Erlang switches maps to a hashmap above 32 keys, and `Enum.with_index/2`
      # is not guaranteed to visit keys in the same order as `Map.values/1`.
      # The old implementation built the SET clause from one and the values from
      # the other, so a wide record could bind a column to a different column's
      # value. `listings` (45 columns) hit this in production.
      name = cname("wide")
      width = 40

      fields =
        for i <- 1..width do
          %{"name" => "col_#{i}", "type" => "text"}
        end

      {:ok, _} = DDL.create_collection(name, type: "base", fields: fields)
      Registry.reload!()

      inserted = Map.new(1..width, fn i -> {"col_#{i}", "value-#{i}"} end)
      {:ok, record} = GenericRecord.insert(name, inserted)

      # The controller hands `update/3` the whole record (hooks may mutate any
      # field), so reproduce that shape: every column present, one changed.
      attrs =
        record
        |> Map.drop(["id", "created_at", "updated_at"])
        |> Map.put("col_17", "changed")

      {:ok, updated} = GenericRecord.update(name, record["id"], attrs)

      for i <- 1..width do
        expected = if i == 17, do: "changed", else: "value-#{i}"

        assert updated["col_#{i}"] == expected,
               "col_#{i} bound to the wrong value: #{inspect(updated["col_#{i}"])}"
      end
    end

    test "pairs values correctly when column types differ across a wide update" do
      # Mixed types make a desync loud: a value bound to the wrong column can
      # no longer be encoded and raises instead of corrupting data.
      name = cname("mixed")

      fields =
        for i <- 1..14 do
          [
            %{"name" => "text_#{i}", "type" => "text"},
            %{"name" => "num_#{i}", "type" => "number"},
            %{"name" => "when_#{i}", "type" => "datetime"}
          ]
        end
        |> List.flatten()

      {:ok, _} = DDL.create_collection(name, type: "base", fields: fields)
      Registry.reload!()

      inserted =
        Map.new(1..14, fn i ->
          {"text_#{i}", "t#{i}"}
        end)
        |> Map.merge(Map.new(1..14, fn i -> {"num_#{i}", i} end))

      {:ok, record} = GenericRecord.insert(name, inserted)

      attrs = record |> Map.drop(["id", "created_at", "updated_at"]) |> Map.put("num_7", 777)
      {:ok, updated} = GenericRecord.update(name, record["id"], attrs)

      for i <- 1..14 do
        expected = if i == 7, do: 777, else: i
        assert updated["num_#{i}"] == expected
        assert updated["text_#{i}"] == "t#{i}"
      end
    end

    test "get/3 returns nil for an id that is not a uuid" do
      name = create_autodate_collection()
      assert GenericRecord.get(name, "not-a-uuid") == nil
    end
  end
end
