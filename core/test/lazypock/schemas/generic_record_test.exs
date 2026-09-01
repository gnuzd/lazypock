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

      updated = GenericRecord.update(name, record["id"], %{"title" => "v2"})

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

      updated =
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

      updated =
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
      updated = GenericRecord.update(name, record["id"], %{"title" => "ho"})
      assert recent?(updated["updated_at"])
      assert updated["title"] == "ho"
    end
  end
end
