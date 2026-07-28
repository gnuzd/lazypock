defmodule LazypockWeb.DynamicControllerTest do
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Schema.DDL
  alias Lazypock.Repo
  alias Lazypock.Auth.SuperUser
  alias Lazypock.Auth.Token
  alias Lazypock.Schemas.GenericRecord
  alias Lazypock.Collections.Registry

  @collection "test_items"
  @fields [
    %{"name" => "title", "type" => "text", "required" => false, "indexed" => false},
    %{"name" => "count", "type" => "number", "required" => false, "indexed" => false},
    %{"name" => "active", "type" => "bool", "required" => false, "indexed" => false}
  ]

  setup do
    # Create test collection
    {:ok, _coll} = DDL.create_collection(@collection, type: "base", fields: @fields)

    # Reload registry so it sees the new collection
    Registry.reload!()

    :ok
  end

  defp auth_token do
    superuser = %SuperUser{
      id: Ecto.UUID.generate(),
      email: "admin@test.com",
      password_hash: Bcrypt.hash_pwd_salt("password")
    }
    Repo.insert!(superuser)
    {:ok, token} = Token.generate_access_token(superuser)
    token
  end

  defp auth_conn(conn) do
    put_req_header(conn, "authorization", "Bearer #{auth_token()}")
  end

  defp insert_test_item(attrs) do
    default = %{"title" => "test item", "count" => 10, "active" => true}
    {:ok, record} = GenericRecord.insert(@collection, Map.merge(default, attrs))
    record
  end

  defp list_path, do: "/api/#{@collection}"
  defp item_path(id), do: "/api/#{@collection}/#{id}"

  # ── List ─────────────────────────────────────────────────

  describe "GET /api/:collection — list" do
    test "returns empty list when no records exist" do
      conn = get(build_conn(), list_path())
      assert json_response(conn, 200)["totalItems"] == 0
      assert json_response(conn, 200)["items"] == []
    end

    test "returns records after creating some" do
      insert_test_item(%{"title" => "first"})
      insert_test_item(%{"title" => "second"})

      conn = get(build_conn(), list_path())
      body = json_response(conn, 200)
      assert body["totalItems"] == 2
      assert length(body["items"]) == 2
    end

    test "filters with ?filter=count>5" do
      insert_test_item(%{"title" => "low", "count" => 3})
      insert_test_item(%{"title" => "high", "count" => 10})

      conn = get(build_conn(), list_path(), %{filter: "count > 5"})
      body = json_response(conn, 200)
      assert body["totalItems"] == 1
      assert hd(body["items"])["title"] == "high"
    end

    test "sorts with ?sort=-count" do
      insert_test_item(%{"title" => "a", "count" => 1})
      insert_test_item(%{"title" => "b", "count" => 5})
      insert_test_item(%{"title" => "c", "count" => 3})

      conn = get(build_conn(), list_path(), %{sort: "-count"})
      body = json_response(conn, 200)
      items = body["items"]
      assert hd(items)["count"] == 5
      assert List.last(items)["count"] == 1
    end

    test "paginates with ?page=1&perPage=2" do
      for i <- 1..5 do
        insert_test_item(%{"title" => "item-#{i}", "count" => i})
      end

      conn = get(build_conn(), list_path(), %{page: "1", perPage: "2"})
      body = json_response(conn, 200)
      assert body["perPage"] == 2
      assert length(body["items"]) == 2
      assert body["totalItems"] == 5
      assert body["totalPages"] == 3
    end

    test "skips total count with ?skipTotal=true" do
      insert_test_item(%{"title" => "x"})

      conn = get(build_conn(), list_path(), %{skipTotal: "true"})
      body = json_response(conn, 200)
      assert body["totalItems"] == 0
      assert length(body["items"]) == 1
    end
  end

  # ── Show ─────────────────────────────────────────────────

  describe "GET /api/:collection/:id — show" do
    test "returns the record by id" do
      record = insert_test_item(%{"title" => "show-me"})

      conn = get(build_conn(), item_path(record["id"]))
      body = json_response(conn, 200)
      assert body["id"] == record["id"]
      assert body["title"] == "show-me"
      assert body["count"] == 10
      assert body["active"] == true
    end

    test "returns 404 for unknown id" do
      conn = get(build_conn(), item_path(Ecto.UUID.generate()))
      assert json_response(conn, 404)
    end
  end

  # ── Create ───────────────────────────────────────────────

  describe "POST /api/:collection — create" do
    test "creates a record with valid data and returns PocketBase-compatible response" do
      conn =
        build_conn()
        |> auth_conn()
        |> post(list_path(), %{data: %{title: "format-check", count: 99, active: false}})

      body = json_response(conn, 201)
      assert body["title"] == "format-check"
      assert body["count"] == 99
      assert body["active"] == false
      assert body["id"] != nil
      assert body["collectionId"] != nil
      assert body["collectionName"] == @collection
      assert body["created"] != nil
      assert body["updated"] != nil
    end

    test "fails without auth token" do
      conn =
        build_conn()
        |> post(list_path(), %{data: %{title: "no-auth"}})

      # Without auth, the enforcer checks rules; default createRule is "" (public)
      # Actually default for base collections is "@request.auth.id != ''" which
      # means anonymous users are DENIED for create.
      assert response(conn, 403)
    end
  end

  # ── Update ───────────────────────────────────────────────

  describe "PATCH /api/:collection/:id — update" do
    test "updates a record and returns it" do
      record = insert_test_item(%{"title" => "old title", "count" => 1})

      conn =
        build_conn()
        |> auth_conn()
        |> patch(item_path(record["id"]), %{data: %{title: "updated title", count: 99}})

      body = json_response(conn, 200)
      assert body["title"] == "updated title"
      assert body["count"] == 99
      assert body["id"] == record["id"]
    end

    test "returns 404 for unknown id" do
      conn =
        build_conn()
        |> auth_conn()
        |> patch(item_path(Ecto.UUID.generate()), %{data: %{title: "ghost"}})

      assert response(conn, 404)
    end
  end

  # ── Delete ───────────────────────────────────────────────

  describe "DELETE /api/:collection/:id — delete" do
    test "deletes a record and returns 204" do
      record = insert_test_item(%{"title" => "delete-me"})

      conn =
        build_conn()
        |> auth_conn()
        |> delete(item_path(record["id"]))

      assert response(conn, 204)

      # Verify it's gone
      assert GenericRecord.get(@collection, record["id"]) == nil
    end

    test "returns 404 for unknown id" do
      conn =
        build_conn()
        |> auth_conn()
        |> delete(item_path(Ecto.UUID.generate()))

      assert response(conn, 404)
    end
  end

  # ── Auth integration ─────────────────────────────────────

  describe "auth integration" do
    test "unauthenticated list request is public (empty listRule)" do
      insert_test_item(%{"title" => "public"})
      # Default listRule for base collections is "" (public)
      conn = get(build_conn(), list_path())
      assert json_response(conn, 200)["totalItems"] == 1
    end

    test "authenticated request via superuser token works" do
      insert_test_item(%{"title" => "auth-check"})

      conn =
        build_conn()
        |> auth_conn()
        |> get(list_path())

      assert json_response(conn, 200)["totalItems"] == 1
    end
  end

  # ── Error handling ───────────────────────────────────────

  describe "error handling" do
    test "404 for non-existent collection" do
      conn = get(build_conn(), "/api/nonexistent")
      assert json_response(conn, 404)
    end

    test "404 for non-existent collection on show" do
      conn = get(build_conn(), "/api/nonexistent/abc-123")
      assert json_response(conn, 404)
    end

    test "404 for non-existent collection on create" do
      conn =
        build_conn()
        |> auth_conn()
        |> post("/api/nonexistent", %{data: %{title: "x"}})

      assert response(conn, 404)
    end

    test "404 for non-existent collection on update" do
      conn =
        build_conn()
        |> auth_conn()
        |> patch("/api/nonexistent/abc-123", %{data: %{title: "x"}})

      assert response(conn, 404)
    end

    test "404 for non-existent collection on delete" do
      conn =
        build_conn()
        |> auth_conn()
        |> delete("/api/nonexistent/abc-123")

      assert response(conn, 404)
    end
  end
end
