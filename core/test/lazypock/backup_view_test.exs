defmodule Lazypock.BackupViewTest do
  use Lazypock.DataCase, async: false

  alias Lazypock.Schema.DDL
  alias Lazypock.Schemas.GenericRecord
  alias Lazypock.Backup
  alias Lazypock.Collections.Registry

  defp cname(prefix), do: "#{prefix}_#{System.unique_integer([:positive]) |> abs()}"

  test "export → restore round-trips view collections (created after their sources)" do
    src = cname("src")
    view = cname("view")

    {:ok, _} =
      DDL.create_collection(src,
        type: "base",
        fields: [
          %{"name" => "title", "type" => "text", "required" => false},
          %{"name" => "count", "type" => "number", "required" => false}
        ]
      )

    {:ok, record} = GenericRecord.insert(src, %{"title" => "hello", "count" => 2})

    {:ok, _} =
      DDL.create_collection(view,
        type: "view",
        options: %{"view_query" => "SELECT id, title, count FROM #{src}"},
        rules: %{"listRule" => "", "viewRule" => ""}
      )

    # Capture the payload, then simulate a fresh restore target: drop
    # everything before restoring.
    payload = Backup.export()
    DDL.drop_collection(view)
    DDL.drop_collection(src)

    assert %{imported: imported, errors: errors} = Backup.restore(payload)

    assert errors == []

    assert Enum.find(imported, &(&1.name == src)).records_imported == 1
    assert Enum.find(imported, &(&1.name == view)).type == "view"

    Registry.reload!()

    # The restored view resolves against the restored source and shows rows.
    {:ok, coll} = Registry.get(view)
    assert coll.type == "view"
    assert coll.options["view_query"] =~ src

    rows = GenericRecord.all(view)
    assert length(rows) == 1
    assert hd(rows)["title"] == "hello"

    # Source records are restored too.
    assert GenericRecord.get(src, record["id"]) != nil
  end
end
