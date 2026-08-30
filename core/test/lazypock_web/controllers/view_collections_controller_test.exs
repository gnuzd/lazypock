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
end
