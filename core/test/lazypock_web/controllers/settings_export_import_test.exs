defmodule LazypockWeb.SettingsExportImportTest do
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Schema.DDL
  alias Lazypock.Repo
  alias Lazypock.Auth.SuperUser
  alias Lazypock.Auth.Token
  alias Lazypock.Schemas.GenericRecord
  alias Lazypock.Collections.Registry

  defp random_name(prefix) do
    suffix = :crypto.strong_rand_bytes(4) |> Base.encode16() |> String.downcase()
    prefix <> suffix
  end

  defp auth_conn(conn) do
    email = "admin_#{:erlang.unique_integer([:positive])}@test.com"

    superuser = %SuperUser{
      id: Ecto.UUID.generate(),
      email: email,
      password_hash: Bcrypt.hash_pwd_salt("password")
    }

    Repo.insert!(superuser)
    {:ok, token} = Token.generate_access_token(superuser)
    put_req_header(conn, "authorization", "Bearer #{token}")
  end

  defp json_post(conn, path, payload) do
    conn
    |> put_req_header("content-type", "application/json")
    |> post(path, Jason.encode!(payload))
  end

  defp title_field do
    %{"name" => "title", "type" => "text", "required" => false, "indexed" => false}
  end

  defp custom_rules do
    %{
      "listRule" => "title != ''",
      "viewRule" => "title != ''",
      "createRule" => "@request.auth.id != ''",
      "updateRule" => "",
      "deleteRule" => "",
      "manageRule" => nil
    }
  end

  describe "GET /api/export" do
    test "requires superuser" do
      conn = get(build_conn(), "/api/export")
      assert json_response(conn, 403)
    end

    test "includes schema, rules, options, hooks and all records" do
      name = random_name("exp_full_")
      {:ok, _} = DDL.create_collection(name, type: "base", fields: [title_field()])
      {:ok, _} = GenericRecord.insert(name, %{"title" => "hello"})
      Registry.reload!()

      conn = get(auth_conn(build_conn()), "/api/export")
      body = json_response(conn, 200)

      coll = Enum.find(body["collections"], &(&1["name"] == name))
      assert coll["type"] == "base"
      assert Enum.any?(coll["schema"], &(&1["name"] == "title"))
      assert is_map(coll["rules"])
      assert is_map(coll["options"])
      assert is_map(coll["hooks"])
      assert length(coll["records"]) == 1
      assert hd(coll["records"])["title"] == "hello"
    end
  end

  describe "POST /api/import" do
    test "creates a new collection with rules, options, hooks and records" do
      name = random_name("imp_new_")

      payload = %{
        collections: [
          %{
            name: name,
            type: "base",
            schema: [title_field()],
            rules: custom_rules(),
            options: %{"indexes" => []},
            hooks: %{"onRecordCreate" => "custom-hook"},
            records: [%{"title" => "imported"}, %{"title" => "second"}]
          }
        ]
      }

      conn = json_post(auth_conn(build_conn()), "/api/import", payload)
      body = json_response(conn, 200)

      assert body["errors"] == []
      assert [%{"name" => ^name, "records_imported" => 2}] = body["imported"]

      Registry.reload!()
      {:ok, coll} = Registry.get(name)
      assert coll.rules["listRule"] == "title != ''"
      assert coll.hooks["onRecordCreate"] == "custom-hook"

      records = GenericRecord.all(name)
      assert Enum.map(records, & &1["title"]) |> Enum.sort() == ["imported", "second"]
    end

    test "updates rules/options/hooks on an existing collection" do
      name = random_name("imp_upd_")
      {:ok, _} = DDL.create_collection(name, type: "base", fields: [title_field()])
      {:ok, _} = GenericRecord.insert(name, %{"title" => "keep"})
      Registry.reload!()

      payload = %{
        collections: [
          %{
            name: name,
            type: "base",
            schema: [title_field()],
            rules: custom_rules(),
            options: %{"indexes" => []},
            hooks: %{"onRecordUpdate" => "x"},
            records: []
          }
        ]
      }

      conn = json_post(auth_conn(build_conn()), "/api/import", payload)
      body = json_response(conn, 200)
      assert body["errors"] == []

      Registry.reload!()
      {:ok, coll} = Registry.get(name)
      assert coll.rules["listRule"] == "title != ''"
      assert coll.hooks["onRecordUpdate"] == "x"

      # records untouched when payload has none
      assert length(GenericRecord.all(name)) == 1
    end

    test "export → import round-trip preserves rules, options and hooks" do
      name = random_name("exp_rt_")
      {:ok, _} = DDL.create_collection(name, type: "base", fields: [title_field()])
      {:ok, _} = DDL.update_collection(name, rules: custom_rules())
      {:ok, _} = GenericRecord.insert(name, %{"title" => "roundtrip"})
      Registry.reload!()

      export_body =
        build_conn()
        |> auth_conn()
        |> get("/api/export")
        |> json_response(200)

      coll_payload = Enum.find(export_body["collections"], &(&1["name"] == name))

      conn = json_post(auth_conn(build_conn()), "/api/import", %{collections: [coll_payload]})
      body = json_response(conn, 200)
      assert body["errors"] == []

      Registry.reload!()
      {:ok, coll} = Registry.get(name)
      assert coll.rules == coll_payload["rules"]
      assert coll.options == coll_payload["options"]
      assert coll.hooks == coll_payload["hooks"]
      # import re-inserts payload records (no dedupe): 1 original + 1 re-imported
      assert length(GenericRecord.all(name)) == 2
      assert [%{"title" => "roundtrip"} | _] = GenericRecord.all(name)
    end

    test "imports a PocketBase-style export (camelCase fields, PB collection ids)" do
      payload = %{
        collections: [
          %{
            id: "kanban_columns_col",
            name: "columns",
            type: "base",
            system: false,
            schema: [
              %{id: "col_title_field", name: "title", type: "text", required: true,
                options: %{"min" => 1, "max" => 100}},
              %{id: "col_order_field", name: "order", type: "number", required: true,
                options: %{"min" => 0, "max" => 1000}}
            ],
            indexes: [],
            listRule: "@request.auth.id != ''",
            viewRule: "@request.auth.id != ''",
            createRule: "@request.auth.id != ''",
            updateRule: "@request.auth.id != ''",
            deleteRule: "@request.auth.id != ''"
          },
          %{
            id: "kanban_cards_col",
            name: "cards",
            type: "base",
            system: false,
            schema: [
              %{id: "card_title_field", name: "title", type: "text", required: true,
                options: %{"min" => 1, "max" => 255}},
              %{id: "card_tagcolor_field", name: "tagColor", type: "select",
                options: %{"maxSelect" => 1, "values" => ["primary", "accent", "success"]}},
              %{id: "card_column_rel", name: "column", type: "relation", required: true,
                options: %{"collectionId" => "kanban_columns_col", "cascadeDelete" => true,
                  "maxSelect" => 1, "displayFields" => ["title"]}},
              %{id: "card_assignee_rel", name: "assignee", type: "relation",
                options: %{"collectionId" => "pb_users_auth", "maxSelect" => 1}}
            ],
            indexes: [],
            listRule: "@request.auth.id != ''",
            viewRule: "@request.auth.id != ''",
            createRule: "@request.auth.id != ''",
            updateRule: "@request.auth.id != ''",
            deleteRule: "@request.auth.id != ''"
          }
        ]
      }

      conn = json_post(auth_conn(build_conn()), "/api/import", payload)
      body = json_response(conn, 200)
      assert body["errors"] == []
      assert Enum.map(body["imported"], & &1["name"]) |> Enum.sort() == ["cards", "columns"]

      Registry.reload!()

      # camelCase field normalized to snake_case
      {:ok, cards} = Registry.get("cards")
      assert Enum.any?(cards.fields, &(&1.name == "tag_color"))
      refute Enum.any?(cards.fields, &(&1.name == "tagColor"))

      # relations resolved from PB collection ids to LazyPock collection names
      column = Enum.find(cards.fields, &(&1.name == "column"))
      assert column.options["collection"] == "columns"
      assignee = Enum.find(cards.fields, &(&1.name == "assignee"))
      assert assignee.options["collection"] == "users"

      {:ok, columns} = Registry.get("columns")
      assert Enum.any?(columns.fields, &(&1.name == "title"))
    end

    test "deleteMissing=false keeps fields absent from the payload" do
      name = random_name("imp_keep_")

      {:ok, _} =
        DDL.create_collection(name, type: "base",
          fields: [title_field(), %{"name" => "extra", "type" => "text", "required" => false}]
        )

      Registry.reload!()

      conn =
        json_post(auth_conn(build_conn()), "/api/import", %{
          collections: [%{name: name, type: "base", schema: [title_field()]}],
          deleteMissing: false
        })

      body = json_response(conn, 200)
      assert body["errors"] == []

      Registry.reload!()
      {:ok, coll} = Registry.get(name)
      assert Enum.any?(coll.fields, &(&1.name == "extra")), "extra field must survive"
      assert Enum.any?(coll.fields, &(&1.name == "title"))
    end

    test "deleteMissing=true drops fields absent from the payload" do
      name = random_name("imp_drop_")

      {:ok, _} =
        DDL.create_collection(name, type: "base",
          fields: [title_field(), %{"name" => "extra", "type" => "text", "required" => false}]
        )

      Registry.reload!()

      conn =
        json_post(auth_conn(build_conn()), "/api/import", %{
          collections: [%{name: name, type: "base", schema: [title_field()]}],
          deleteMissing: true
        })

      body = json_response(conn, 200)
      assert body["errors"] == []

      Registry.reload!()
      {:ok, coll} = Registry.get(name)
      refute Enum.any?(coll.fields, &(&1.name == "extra")), "extra field must be dropped"
    end

    test "deleteMissing=true drops user collections but never system collections" do
      name = random_name("imp_sys_")
      {:ok, _} = DDL.create_collection(name, type: "base", fields: [title_field()])
      Registry.reload!()

      conn =
        json_post(auth_conn(build_conn()), "/api/import", %{
          collections: [],
          deleteMissing: true
        })

      body = json_response(conn, 200)
      assert body["errors"] == []

      Registry.reload!()
      assert Registry.get(name) == {:error, :not_found}
      assert {:ok, users} = Registry.get("users")
      assert users.system
    end

    test "imports the exact PocketBase kanban export fixture with deleteMissing=false" do
      path = Path.expand("../../fixtures/pocketbase_kanban_import.json", __DIR__)
      collections = Jason.decode!(File.read!(path))

      conn =
        json_post(auth_conn(build_conn()), "/api/import", %{
          collections: collections,
          deleteMissing: false
        })

      body = json_response(conn, 200)
      assert body["errors"] == []
      assert Enum.map(body["imported"], & &1["name"]) |> Enum.sort() == ["cards", "columns"]

      Registry.reload!()

      {:ok, cards} = Registry.get("cards")
      assert Enum.map(cards.fields, & &1.name) |> Enum.sort() ==
               ["assignee", "column", "description", "order", "tag", "tag_color", "title"]

      column = Enum.find(cards.fields, &(&1.name == "column"))
      assert column.options["collection"] == "columns"
      assert column.options["cascadeDelete"] == true
      assert column.options["displayFields"] == ["title"]

      assignee = Enum.find(cards.fields, &(&1.name == "assignee"))
      assert assignee.options["collection"] == "users"

      {:ok, columns} = Registry.get("columns")
      assert Enum.map(columns.fields, & &1.name) |> Enum.sort() == ["order", "title"]

      # Re-import with deleteMissing=false: nothing is dropped or duplicated
      conn =
        json_post(auth_conn(build_conn()), "/api/import", %{
          collections: collections,
          deleteMissing: false
        })

      body = json_response(conn, 200)
      assert body["errors"] == []

      Registry.reload!()
      {:ok, cards} = Registry.get("cards")
      assert Enum.map(cards.fields, & &1.name) |> Enum.sort() ==
               ["assignee", "column", "description", "order", "tag", "tag_color", "title"]
    end
  end
end
