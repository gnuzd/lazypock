defmodule LazypockWeb.ViewCollectionsControllerTest do
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Schema.DDL
  alias Lazypock.Repo
  alias Lazypock.Auth.SuperUser
  alias Lazypock.Auth.Token
  alias Lazypock.Schemas.GenericRecord
  alias Lazypock.Collections.Registry

  setup do
    src = collection_name()

    {:ok, _coll} =
      DDL.create_collection(src,
        type: "base",
        fields: [
          %{"name" => "title", "type" => "text", "required" => false},
          %{"name" => "count", "type" => "number", "required" => false}
        ]
      )

    Registry.reload!()
    {:ok, src: src}
  end

  defp collection_name do
    "src_#{:erlang.unique_integer([:positive])}"
  end

  defp auth_token do
    superuser = %SuperUser{
      id: Ecto.UUID.generate(),
      email: "admin_#{System.unique_integer([:positive])}@test.com",
      password_hash: Bcrypt.hash_pwd_salt("password")
    }

    Repo.insert!(superuser)
    {:ok, token} = Token.generate_access_token(superuser)
    token
  end

  defp auth_conn(conn) do
    put_req_header(conn, "authorization", "Bearer #{auth_token()}")
  end

  defp json_post(conn, path, payload) do
    conn
    |> put_req_header("content-type", "application/json")
    |> post(path, Jason.encode!(payload))
  end

  defp json_patch(conn, path, payload) do
    conn
    |> put_req_header("content-type", "application/json")
    |> patch(path, Jason.encode!(payload))
  end

  describe "view collections API" do
    test "creates a view collection via the API with viewQuery", %{src: src} do
      name = "view_" <> Integer.to_string(:erlang.unique_integer([:positive]))

      conn = auth_conn(build_conn())

      conn =
        post(conn, "/api/collections",
          name: name,
          type: "view",
          viewQuery: "SELECT id, title FROM #{src}"
        )

      assert %{"type" => "view", "viewQuery" => query} = json_response(conn, 201)
      assert query =~ src

      assert [%{"name" => "id"}, %{"name" => "title"}] =
               json_response(conn, 201)["fields"]
    end

    test "updates a view collection when the client echoes the auto-generated fields", %{
      src: src
    } do
      name = "view_" <> Integer.to_string(:erlang.unique_integer([:positive]))

      conn =
        auth_conn(build_conn())
        |> post("/api/collections",
          name: name,
          type: "view",
          viewQuery: "SELECT id, title FROM #{src}"
        )

      assert %{"id" => id, "fields" => fields} = json_response(conn, 201)

      # The Studio always posts the full field list back on save, even though
      # view fields are derived from the query server-side.
      conn =
        auth_conn(build_conn())
        |> patch("/api/collections/#{id}",
          name: name,
          type: "view",
          viewQuery: "SELECT id, title, count FROM #{src}",
          fields: fields
        )

      assert %{"type" => "view", "viewQuery" => query, "fields" => new_fields} =
               json_response(conn, 200)

      assert query =~ "count"
      assert Enum.map(new_fields, & &1["name"]) == ["id", "title", "count"]
    end

    test "rejects a view collection with an invalid query" do
      conn = auth_conn(build_conn())

      conn =
        post(conn, "/api/collections",
          name: "bad_view_" <> Integer.to_string(:erlang.unique_integer([:positive])),
          type: "view",
          viewQuery: "SELECT title FROM missing_table_xyz"
        )

      assert %{"error" => error} = json_response(conn, 400)
      assert error =~ "missing_table_xyz"
    end

    test "dry-run-view returns fields and a sample", %{src: src} do
      {:ok, _} = GenericRecord.insert(src, %{"title" => "hello", "count" => 1})

      conn = auth_conn(build_conn())

      conn =
        post(conn, "/api/collections/meta/dry-run-view", query: "SELECT id, title FROM #{src}")

      assert %{"fields" => fields, "sample" => sample} = json_response(conn, 200)
      assert Enum.map(fields, & &1["name"]) == ["id", "title"]
      assert hd(sample)["title"] == "hello"
    end

    test "dry-run-view reports invalid queries" do
      conn = auth_conn(build_conn())

      conn =
        post(conn, "/api/collections/meta/dry-run-view", query: "SELECT * FROM whatever")

      assert %{"message" => message} = json_response(conn, 400)
      assert message =~ "Invalid view query"
    end

    test "requires superuser for dry-run-view" do
      conn = build_conn()
      conn = post(conn, "/api/collections/meta/dry-run-view", query: "SELECT 1")
      assert json_response(conn, 403)
    end
  end

  describe "view builder API" do
    test "creates a view from a builder spec and reports its origin", %{src: src} do
      name = "vb_" <> Integer.to_string(:erlang.unique_integer([:positive]))

      conn =
        auth_conn(build_conn())
        |> json_post("/api/collections", %{
          "name" => name,
          "type" => "view",
          "viewBuilder" => %{
            "source" => src,
            "fields" => [%{"name" => "title"}],
            "sort" => "-title"
          }
        })

      body = json_response(conn, 201)
      assert body["type"] == "view"
      assert body["viewOrigin"] == "builder"
      assert body["viewBuilder"]["source"] == src
      assert body["viewQuery"] =~ src
      assert body["viewQuery"] =~ "ORDER BY"
      assert Enum.map(body["fields"], & &1["name"]) == ["id", "title"]
    end

    test "ignores a client viewQuery when a builder spec is present", %{src: src} do
      name = "vb_" <> Integer.to_string(:erlang.unique_integer([:positive]))

      conn =
        auth_conn(build_conn())
        |> json_post("/api/collections", %{
          "name" => name,
          "type" => "view",
          "viewQuery" => "SELECT 1 AS id",
          "viewBuilder" => %{"source" => src, "fields" => [%{"name" => "title"}]}
        })

      body = json_response(conn, 201)
      assert body["viewQuery"] =~ src
      refute body["viewQuery"] =~ "SELECT 1 AS id"
    end

    test "regenerates the query and fields when the builder spec changes", %{src: src} do
      name = "vb_" <> Integer.to_string(:erlang.unique_integer([:positive]))

      conn =
        auth_conn(build_conn())
        |> json_post("/api/collections", %{
          "name" => name,
          "type" => "view",
          "viewBuilder" => %{"source" => src, "fields" => [%{"name" => "title"}]}
        })

      %{"id" => id} = json_response(conn, 201)

      conn =
        auth_conn(build_conn())
        |> json_patch("/api/collections/#{id}", %{
          "name" => name,
          "type" => "view",
          "viewBuilder" => %{
            "source" => src,
            "fields" => [%{"name" => "title"}, %{"name" => "count"}]
          }
        })

      body = json_response(conn, 200)
      assert body["viewOrigin"] == "builder"
      assert Enum.map(body["fields"], & &1["name"]) == ["id", "title", "count"]
    end

    test "allows updating a builder view's rules without touching the spec", %{src: src} do
      name = "vb_" <> Integer.to_string(:erlang.unique_integer([:positive]))

      conn =
        auth_conn(build_conn())
        |> json_post("/api/collections", %{
          "name" => name,
          "type" => "view",
          "viewBuilder" => %{"source" => src, "fields" => [%{"name" => "title"}]}
        })

      %{"id" => id, "viewQuery" => query} = json_response(conn, 201)

      conn =
        auth_conn(build_conn())
        |> json_patch("/api/collections/#{id}", %{
          "name" => name,
          "type" => "view",
          "listRule" => ""
        })

      body = json_response(conn, 200)
      assert body["viewOrigin"] == "builder"
      assert body["viewQuery"] == query
      assert body["viewBuilder"]["source"] == src
      assert body["rules"]["listRule"] == ""
    end

    test "rejects a builder payload on a SQL-created view", %{src: src} do
      name = "vb_" <> Integer.to_string(:erlang.unique_integer([:positive]))

      conn =
        auth_conn(build_conn())
        |> json_post("/api/collections", %{
          "name" => name,
          "type" => "view",
          "viewQuery" => "SELECT id, title FROM #{src}"
        })

      %{"id" => id, "viewOrigin" => "sql"} = json_response(conn, 201)

      conn =
        auth_conn(build_conn())
        |> json_patch("/api/collections/#{id}", %{
          "name" => name,
          "type" => "view",
          "viewBuilder" => %{"source" => src, "fields" => [%{"name" => "title"}]}
        })

      assert %{"error" => error} = json_response(conn, 400)
      assert error =~ "created with SQL"
    end

    test "keeps a SQL view's origin and updates its raw query" do
      src = "src_#{:erlang.unique_integer([:positive])}"

      {:ok, _} =
        DDL.create_collection(src,
          type: "base",
          fields: [%{"name" => "title", "type" => "text", "required" => false}]
        )

      name = "vb_" <> Integer.to_string(:erlang.unique_integer([:positive]))

      conn =
        auth_conn(build_conn())
        |> json_post("/api/collections", %{
          "name" => name,
          "type" => "view",
          "viewQuery" => "SELECT id, title FROM #{src}"
        })

      %{"id" => id, "viewOrigin" => "sql"} = json_response(conn, 201)

      conn =
        auth_conn(build_conn())
        |> json_patch("/api/collections/#{id}", %{
          "name" => name,
          "type" => "view",
          "viewQuery" => "SELECT id, title, title AS label FROM #{src}"
        })

      body = json_response(conn, 200)
      assert body["viewOrigin"] == "sql"
      assert body["viewBuilder"] == nil
      assert Enum.map(body["fields"], & &1["name"]) == ["id", "title", "label"]
    end

    test "rejects a builder spec that references an unknown field", %{src: src} do
      name = "vb_" <> Integer.to_string(:erlang.unique_integer([:positive]))

      conn =
        auth_conn(build_conn())
        |> json_post("/api/collections", %{
          "name" => name,
          "type" => "view",
          "viewBuilder" => %{"source" => src, "fields" => [%{"name" => "missing"}]}
        })

      assert %{"error" => error} = json_response(conn, 400)
      assert error =~ "missing"
    end

    test "previews a builder spec with generated SQL, fields and sample", %{src: src} do
      {:ok, _} = GenericRecord.insert(src, %{"title" => "hello", "count" => 1})

      conn =
        auth_conn(build_conn())
        |> json_post("/api/collections/meta/preview-view-builder", %{
          "viewBuilder" => %{"source" => src, "fields" => [%{"name" => "title"}]}
        })

      body = json_response(conn, 200)
      assert body["query"] =~ src
      assert Enum.map(body["fields"], & &1["name"]) == ["id", "title"]
      assert [%{"title" => "hello"}] = body["sample"]
    end

    test "preview rejects an invalid builder spec", %{src: src} do
      conn =
        auth_conn(build_conn())
        |> json_post("/api/collections/meta/preview-view-builder", %{
          "viewBuilder" => %{"source" => src, "fields" => [%{"name" => "missing"}]}
        })

      assert %{"message" => message} = json_response(conn, 400)
      assert message =~ "missing"
    end

    test "preview requires a viewBuilder param" do
      conn =
        auth_conn(build_conn())
        |> json_post("/api/collections/meta/preview-view-builder", %{})

      assert %{"message" => message} = json_response(conn, 400)
      assert message =~ "Missing viewBuilder"
    end

    test "requires superuser for the builder preview" do
      conn =
        json_post(build_conn(), "/api/collections/meta/preview-view-builder", %{
          "viewBuilder" => %{}
        })

      assert json_response(conn, 403)
    end
  end

  describe "read-only enforcement" do
    setup %{src: src} do
      name = "ro_view_" <> Integer.to_string(:erlang.unique_integer([:positive]))

      {:ok, _coll} =
        DDL.create_collection(name,
          type: "view",
          options: %{"view_query" => "SELECT id, title FROM #{src}"}
        )

      {:ok, record} = GenericRecord.insert(src, %{"title" => "a", "count" => 1})
      Registry.reload!()
      {:ok, name: name, record: record}
    end

    test "lists and shows view records", %{name: name} do
      conn = auth_conn(build_conn())
      conn = get(conn, "/api/#{name}")
      assert %{"items" => [_], "totalItems" => 1} = json_response(conn, 200)

      id = json_response(conn, 200)["items"] |> hd() |> Map.get("id")
      conn = auth_conn(build_conn())
      conn = get(conn, "/api/#{name}/#{id}")
      assert %{"title" => "a"} = json_response(conn, 200)
    end

    test "rejects create/update/delete on views with 400", %{name: name, record: record} do
      conn = auth_conn(build_conn())

      conn = post(conn, "/api/#{name}", %{"title" => "x"})
      assert %{"message" => "Unsupported collection type."} = json_response(conn, 400)

      conn = auth_conn(build_conn())
      conn = patch(conn, "/api/#{name}/#{record["id"]}", %{"title" => "y"})
      assert %{"message" => "Unsupported collection type."} = json_response(conn, 400)

      conn = auth_conn(build_conn())
      conn = delete(conn, "/api/#{name}/#{record["id"]}")
      assert %{"message" => "Unsupported collection type."} = json_response(conn, 400)
    end

    test "view updates its rows when the source changes", %{name: name, src: src} do
      # Mutation flows through the dynamic controller and should be visible
      # through the view immediately (read path).
      conn = auth_conn(build_conn())
      post(conn, "/api/#{src}", %{"title" => "new-row", "count" => 2})

      conn = auth_conn(build_conn())
      conn = get(conn, "/api/#{name}")
      assert %{"totalItems" => 2} = json_response(conn, 200)
    end
  end

  describe "rules on view records behave like base collections" do
    setup %{src: src} do
      name = "vwrules_" <> Integer.to_string(:erlang.unique_integer([:positive]))

      {:ok, _coll} =
        DDL.create_collection(name,
          type: "view",
          options: %{"view_query" => "SELECT id, title, count FROM #{src}"}
        )

      {:ok, record} = GenericRecord.insert(src, %{"title" => "a", "count" => 1})
      {:ok, other} = GenericRecord.insert(src, %{"title" => "b", "count" => 2})
      Registry.reload!()
      {:ok, name: name, record: record, other: other}
    end

    test "listRule on the view id column filters anonymously like on a base collection", %{
      name: name,
      record: record
    } do
      id = record["id"]

      conn = auth_conn(build_conn())

      conn =
        patch(conn, "/api/collections/#{name}", %{
          "rules" => %{"listRule" => "id = '#{id}'"}
        })

      assert %{"rules" => %{"listRule" => _}} = json_response(conn, 200)

      # Anonymous access honors the rule. A view's id column is TEXT, so the
      # rule must not be cast to uuid (previously that crashed Postgres and
      # the swallowed error returned an empty list).
      conn = build_conn()
      conn = get(conn, "/api/#{name}")
      assert %{"items" => [item], "totalItems" => 1} = json_response(conn, 200)
      assert item["id"] == id
      assert item["title"] == "a"
    end

    test "viewRule with a conditional expression grants show access on views", %{
      name: name,
      record: record,
      other: other
    } do
      conn = auth_conn(build_conn())

      conn =
        patch(conn, "/api/collections/#{name}", %{
          "rules" => %{"viewRule" => "title = 'a'"}
        })

      assert %{"rules" => %{"viewRule" => _}} = json_response(conn, 200)

      conn = build_conn()
      conn = get(conn, "/api/#{name}/#{record["id"]}")
      assert %{"id" => id, "title" => "a"} = json_response(conn, 200)
      assert id == record["id"]

      conn = build_conn()
      conn = get(conn, "/api/#{name}/#{other["id"]}")
      assert %{"message" => "Access denied by viewRule"} = json_response(conn, 403)
    end
  end
end
