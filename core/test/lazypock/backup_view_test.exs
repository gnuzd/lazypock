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

  # ── "fields" vs "schema" key resolution ───────────────────────────────────

  test "restore accepts PocketBase 23+ payloads using the \"fields\" key" do
    name = cname("pb23")

    payload = %{
      "collections" => [
        %{
          "id" => "pbc_1",
          "name" => name,
          "type" => "base",
          "listRule" => "",
          "viewRule" => "",
          "createRule" => nil,
          "updateRule" => nil,
          "deleteRule" => nil,
          "fields" => [
            %{"name" => "title", "type" => "text", "required" => true, "max" => 100}
          ]
        }
      ]
    }

    assert %{imported: imported, errors: []} = Backup.restore(payload)
    assert Enum.find(imported, &(&1.name == name))

    Registry.reload!()
    {:ok, coll} = Registry.get(name)
    assert Enum.any?(coll.fields, &(&1.name == "title"))
  end

  test "restore accepts LazyPock's own payloads using the \"schema\" key" do
    name = cname("lzp")

    payload = %{
      "collections" => [
        %{
          "id" => "pbc_2",
          "name" => name,
          "type" => "base",
          "listRule" => "",
          "viewRule" => "",
          "createRule" => nil,
          "updateRule" => nil,
          "deleteRule" => nil,
          "schema" => [
            %{"name" => "title", "type" => "text", "required" => true, "max" => 100}
          ]
        }
      ]
    }

    assert %{imported: imported, errors: []} = Backup.restore(payload)
    assert Enum.find(imported, &(&1.name == name))
  end

  test "restore rejects a collection missing both \"fields\" and \"schema\"" do
    name = cname("missing_key")

    payload = %{
      "collections" => [
        %{
          "id" => "pbc_3",
          "name" => name,
          "type" => "base",
          "listRule" => "",
          "viewRule" => "",
          "createRule" => nil,
          "updateRule" => nil,
          "deleteRule" => nil
        }
      ]
    }

    assert %{imported: [], errors: [error]} = Backup.restore(payload)
    assert error.name == name
    assert error.error =~ "missing both"

    Registry.reload!()
    assert Registry.get(name) == {:error, :not_found}
  end

  test "restore rejects a collection with both \"fields\" and \"schema\" present" do
    name = cname("both_keys")

    payload = %{
      "collections" => [
        %{
          "id" => "pbc_4",
          "name" => name,
          "type" => "base",
          "listRule" => "",
          "viewRule" => "",
          "createRule" => nil,
          "updateRule" => nil,
          "deleteRule" => nil,
          "fields" => [%{"name" => "a", "type" => "text", "required" => false}],
          "schema" => [%{"name" => "b", "type" => "text", "required" => false}]
        }
      ]
    }

    assert %{imported: [], errors: [error]} = Backup.restore(payload)
    assert error.name == name
    assert error.error =~ "ambiguous"
  end

  test "restore allows an explicitly empty field list without error" do
    name = cname("empty_fields")

    payload = %{
      "collections" => [
        %{
          "id" => "pbc_5",
          "name" => name,
          "type" => "base",
          "listRule" => "",
          "viewRule" => "",
          "createRule" => nil,
          "updateRule" => nil,
          "deleteRule" => nil,
          "fields" => []
        }
      ]
    }

    assert %{imported: imported, errors: []} = Backup.restore(payload)
    assert Enum.find(imported, &(&1.name == name))
  end

  # ── system-field stripping (PocketBase's own "id" field) ──────────────────

  test "restore strips PocketBase's system \"id\" field instead of colliding with LazyPock's own id column" do
    name = cname("pb_system_id")

    payload = %{
      "collections" => [
        %{
          "id" => "pbc_6",
          "name" => name,
          "type" => "base",
          "listRule" => "",
          "viewRule" => "",
          "createRule" => nil,
          "updateRule" => nil,
          "deleteRule" => nil,
          "fields" => [
            %{
              "id" => "text3208210256",
              "name" => "id",
              "type" => "text",
              "system" => true,
              "primaryKey" => true,
              "required" => true,
              "max" => 15,
              "min" => 15
            },
            %{"name" => "title", "type" => "text", "required" => false, "system" => false}
          ]
        }
      ]
    }

    assert %{imported: imported, errors: []} = Backup.restore(payload)
    assert Enum.find(imported, &(&1.name == name))

    Registry.reload!()
    {:ok, coll} = Registry.get(name)
    field_names = Enum.map(coll.fields, & &1.name)

    assert "title" in field_names
    # Only one "id" — LazyPock's own system column, not a second one
    # re-declared from the payload.
    assert Enum.count(field_names, &(&1 == "id")) <= 1
  end

  test "restore keeps non-system fields named \"created\"/\"updated\" that are not marked system" do
    name = cname("explicit_autodate")

    payload = %{
      "collections" => [
        %{
          "id" => "pbc_7",
          "name" => name,
          "type" => "base",
          "listRule" => "",
          "viewRule" => "",
          "createRule" => nil,
          "updateRule" => nil,
          "deleteRule" => nil,
          "fields" => [
            %{
              "name" => "created",
              "type" => "autodate",
              "system" => false,
              "onCreate" => true,
              "onUpdate" => false
            }
          ]
        }
      ]
    }

    assert %{imported: imported, errors: []} = Backup.restore(payload)
    assert Enum.find(imported, &(&1.name == name))

    Registry.reload!()
    {:ok, coll} = Registry.get(name)
    assert Enum.any?(coll.fields, &(&1.name == "created"))
  end

  # ── legacy PocketBase <23 field shape (nested "options") ───────────────────

  test "restore rejects a non-relation field with legacy nested \"options\"" do
    name = cname("legacy_options")

    payload = %{
      "collections" => [
        %{
          "id" => "pbc_8",
          "name" => name,
          "type" => "base",
          "listRule" => "",
          "viewRule" => "",
          "createRule" => nil,
          "updateRule" => nil,
          "deleteRule" => nil,
          "fields" => [
            %{
              "name" => "title",
              "type" => "text",
              "options" => %{"min" => 1, "max" => 100}
            }
          ]
        }
      ]
    }

    assert %{imported: [], errors: [error]} = Backup.restore(payload)
    assert error.name == name
    assert error.error =~ "title"
    assert error.error =~ "PocketBase <23"
  end

  test "restore still accepts a relation field with collectionId nested under \"options\"" do
    target = cname("target")
    source = cname("source")

    {:ok, _} =
      DDL.create_collection(target, type: "base", fields: [])

    payload = %{
      "collections" => [
        %{
          "id" => "pbc_source",
          "name" => source,
          "type" => "base",
          "listRule" => "",
          "viewRule" => "",
          "createRule" => nil,
          "updateRule" => nil,
          "deleteRule" => nil,
          "fields" => [
            %{
              "name" => "ref",
              "type" => "relation",
              "options" => %{"collectionId" => target, "maxSelect" => 1}
            }
          ]
        }
      ]
    }

    # target is matched by name here since we're not passing its LazyPock id
    # through id_to_name in this payload — this exercises the same tolerant
    # collectionId lookup resolve_relation/2 already performs, not a new
    # behavior from this fix.
    assert %{errors: errors} = Backup.restore(payload)
    assert errors == [] or Enum.all?(errors, &(&1.name != source))
  end

  # ── mixed batch: one bad collection doesn't block the rest ────────────────

  test "restore imports valid collections even when another in the same batch is malformed" do
    good = cname("good")
    bad = cname("bad")

    payload = %{
      "collections" => [
        %{
          "id" => "pbc_good",
          "name" => good,
          "type" => "base",
          "listRule" => "",
          "viewRule" => "",
          "createRule" => nil,
          "updateRule" => nil,
          "deleteRule" => nil,
          "fields" => [%{"name" => "title", "type" => "text", "required" => false}]
        },
        %{
          "id" => "pbc_bad",
          "name" => bad,
          "type" => "base",
          "listRule" => "",
          "viewRule" => "",
          "createRule" => nil,
          "updateRule" => nil,
          "deleteRule" => nil
          # no "fields" / "schema" key at all
        }
      ]
    }

    assert %{imported: imported, errors: errors} = Backup.restore(payload)

    assert Enum.find(imported, &(&1.name == good))
    assert Enum.find(errors, &(&1.name == bad))
    refute Enum.find(imported, &(&1.name == bad))

    Registry.reload!()
    assert {:ok, _} = Registry.get(good)
    assert Registry.get(bad) == {:error, :not_found}
  end
end
