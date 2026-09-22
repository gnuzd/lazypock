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

  # ── nested "options" is accepted for every field type ──────────────────────
  #
  # The DDL/field-metadata layer already handles both flat fields and fields
  # with settings nested under "options" — confirmed by the existing
  # PocketBase-import test suite (imports text/number/select fields carrying
  # non-empty "options" successfully). An earlier version of this fix
  # incorrectly rejected non-relation fields with nested "options" as an
  # assumed-unsupported legacy shape; that assumption was never verified
  # against the actual DDL code and directly contradicted the existing test
  # coverage, so it was removed. This test guards against reintroducing it.

  test "restore accepts a non-relation field with nested \"options\" (min/max as settings)" do
    name = cname("nested_options")

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
              "required" => true,
              "options" => %{"min" => 1, "max" => 100}
            }
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

  test "add_field preserves camelCase column name" do
    name = cname("case_col")

    {:ok, _} = DDL.create_collection(name, type: "base", fields: [])
    :ok = DDL.add_field(name, %{"name" => "sortOrder", "type" => "number"})

    {:ok, %{rows: rows}} =
      Lazypock.Repo.query(
        "SELECT column_name FROM information_schema.columns WHERE table_name = $1",
        [name]
      )

    cols = List.flatten(rows)
    assert "sortOrder" in cols
    refute "sortorder" in cols
  end

  test "create_collection preserves camelCase column name" do
    name = cname("case_create")

    {:ok, _} =
      DDL.create_collection(name,
        type: "base",
        fields: [
          %{"name" => "isFeatured", "type" => "bool"},
          %{"name" => "priceOverride", "type" => "number"}
        ]
      )

    {:ok, %{rows: rows}} =
      Lazypock.Repo.query(
        "SELECT column_name FROM information_schema.columns WHERE table_name = $1",
        [name]
      )

    cols = List.flatten(rows)
    assert "isFeatured" in cols
    assert "priceOverride" in cols
    refute "isfeatured" in cols
  end

  # ── select/file with maxSelect > 1 → TEXT[] (bug fix: to_pg_with_opts) ──

  test "restore creates TEXT[] for multi-select and inserts list" do
    name = cname("select_multi")

    payload = %{
      "collections" => [
        %{
          "id" => "pbc_sel",
          "name" => name,
          "type" => "base",
          "fields" => [
            %{
              "name" => "tags",
              "type" => "select",
              "maxSelect" => 3,
              "values" => ["Best Seller", "New", "Sale"]
            }
          ],
          "records" => [%{"tags" => ["Best Seller", "New"]}]
        }
      ]
    }

    assert %{errors: [], imported: [%{records_imported: 1}]} = Backup.restore(payload)

    {:ok, %{rows: [[data_type]]}} =
      Lazypock.Repo.query(
        "SELECT data_type FROM information_schema.columns
         WHERE table_name = $1 AND column_name = 'tags'",
        [name]
      )

    assert data_type == "ARRAY"

    [rec] = GenericRecord.all(name)
    assert rec["tags"] == ["Best Seller", "New"]
  end

  test "restore creates TEXT[] for multi-file" do
    name = cname("file_multi")

    payload = %{
      "collections" => [
        %{
          "id" => "pbc_file",
          "name" => name,
          "type" => "base",
          "fields" => [
            %{
              "name" => "images",
              "type" => "file",
              "maxSelect" => 8,
              "maxSize" => 5_242_880,
              "mimeTypes" => ["image/png", "image/jpeg"]
            }
          ],
          "records" => [%{"images" => ["a.png", "b.jpg"]}]
        }
      ]
    }

    assert %{errors: [], imported: [%{records_imported: 1}]} = Backup.restore(payload)

    {:ok, %{rows: [[data_type]]}} =
      Lazypock.Repo.query(
        "SELECT data_type FROM information_schema.columns
         WHERE table_name = $1 AND column_name = 'images'",
        [name]
      )

    assert data_type == "ARRAY"
    [rec] = GenericRecord.all(name)
    assert rec["images"] == ["a.png", "b.jpg"]
  end

  test "restore keeps TEXT for single-select" do
    name = cname("select_single")

    payload = %{
      "collections" => [
        %{
          "id" => "pbc_sel1",
          "name" => name,
          "type" => "base",
          "fields" => [
            %{"name" => "size", "type" => "select", "maxSelect" => 1, "values" => ["S", "M", "L"]}
          ],
          "records" => [%{"size" => "M"}]
        }
      ]
    }

    assert %{errors: []} = Backup.restore(payload)

    {:ok, %{rows: [[data_type]]}} =
      Lazypock.Repo.query(
        "SELECT data_type FROM information_schema.columns
         WHERE table_name = $1 AND column_name = 'size'",
        [name]
      )

    assert data_type == "text"
  end

  # ── autodate top-level options → nested + stamp (bug fix: normalize_field_options) ──

  test "restore moves autodate top-level onCreate into options and stamps column" do
    name = cname("autodate_pb")

    payload = %{
      "collections" => [
        %{
          "id" => "pbc_auto",
          "name" => name,
          "type" => "base",
          "fields" => [
            %{"name" => "title", "type" => "text", "required" => false},
            %{
              "name" => "lastSeen",
              "type" => "autodate",
              "system" => false,
              "onCreate" => true,
              "onUpdate" => true
            }
          ],
          "records" => []
        }
      ]
    }

    assert %{errors: []} = Backup.restore(payload)

    Registry.reload!()
    {:ok, coll} = Registry.get(name)
    last_seen = Enum.find(coll.fields, &(&1.name == "lastSeen"))
    assert last_seen.options["onCreate"] == true
    assert last_seen.options["onUpdate"] == true

    {:ok, %{rows: [[column_default]]}} =
      Lazypock.Repo.query(
        "SELECT column_default FROM information_schema.columns
         WHERE table_name = $1 AND column_name = 'lastSeen'",
        [name]
      )

    assert column_default =~ "now()"

    {:ok, rec} = GenericRecord.insert(name, %{"title" => "hello"})
    refute is_nil(rec["lastSeen"])
  end

  # ── full integration: camelCase + multi-value + autodate ──

  test "full PocketBase-style restore with camelCase + multi-select + autodate" do
    name = cname("full_rt")

    payload = %{
      "collections" => [
        %{
          "id" => "pbc_full",
          "name" => name,
          "type" => "base",
          "fields" => [
            %{"name" => "name", "type" => "text", "required" => true},
            %{"name" => "priceOverride", "type" => "number"},
            %{"name" => "isFeatured", "type" => "bool"},
            %{
              "name" => "tags",
              "type" => "select",
              "maxSelect" => 3,
              "values" => ["Best Seller", "New", "Sale"]
            },
            %{"name" => "created", "type" => "autodate", "onCreate" => true, "onUpdate" => false}
          ],
          "records" => [
            %{
              "id" => Ecto.UUID.generate(),
              "name" => "Widget",
              "priceOverride" => 9.99,
              "isFeatured" => true,
              "tags" => ["New", "Sale"]
            }
          ]
        }
      ]
    }

    assert %{errors: [], imported: [%{records_imported: 1}]} = Backup.restore(payload)

    {:ok, %{rows: rows}} =
      Lazypock.Repo.query(
        "SELECT column_name FROM information_schema.columns WHERE table_name = $1",
        [name]
      )

    cols = List.flatten(rows)

    for c <- ["name", "priceOverride", "isFeatured", "tags", "created"] do
      assert c in cols, "expected #{c} in #{inspect(cols)}"
    end

    refute "priceoverride" in cols
    refute "isfeatured" in cols

    [rec] = GenericRecord.all(name)
    assert rec["tags"] == ["New", "Sale"]
    assert rec["isFeatured"] == true
    assert rec["priceOverride"] == 9.99
    refute is_nil(rec["created"])

    {:ok, new_rec} =
      GenericRecord.insert(name, %{"name" => "Gadget", "tags" => ["Best Seller"]})

    assert new_rec["tags"] == ["Best Seller"]
    refute is_nil(new_rec["created"])
  end
end
