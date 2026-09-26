defmodule LazypockWeb.RuleSideChannelTest do
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Collections.Collection
  alias Lazypock.Collections.Registry
  alias Lazypock.Repo
  alias Lazypock.Schema.DDL
  alias Lazypock.Schemas.GenericRecord

  # Side-channel / information-leak guarantees for rule enforcement:
  #
  #   * a rule denial must be indistinguishable from a missing record
  #   * a rejected filter must not echo compiler internals or schema details
  #   * a filter naming an unknown field fails closed (no rows), never a 500

  @fields [
    %{"name" => "title", "type" => "text", "required" => false, "indexed" => false},
    %{"name" => "owner_id", "type" => "text", "required" => false, "indexed" => false}
  ]

  defp create_collection(rules) do
    name = "sc_#{System.unique_integer([:positive]) |> abs()}"
    {:ok, coll} = DDL.create_collection(name, type: "base", fields: @fields)

    {:ok, _coll} =
      coll
      |> Collection.changeset(%{rules: Map.merge(coll.rules, rules)})
      |> Repo.update()

    Registry.reload!()
    name
  end

  # Public list, but view requires an authenticated user. Anonymous requests
  # are therefore denied on show, which is all we need to compare the
  # deny-vs-missing response shapes.
  defp public_list_private_view do
    create_collection(%{"listRule" => "", "viewRule" => "@request.auth.id != ''"})
  end

  describe "existence oracle on show" do
    test "a rule-denied record and a missing record are indistinguishable" do
      name = public_list_private_view()
      {:ok, record} = GenericRecord.insert(name, %{"title" => "secret"})

      denied = get(build_conn(), "/api/#{name}/#{record["id"]}")
      missing = get(build_conn(), "/api/#{name}/#{Ecto.UUID.generate()}")

      assert denied.status == 404
      assert missing.status == 404
      assert json_response(denied, 404) == json_response(missing, 404)
    end

    test "a rule denial is never reported as 403 on show" do
      name = public_list_private_view()
      {:ok, record} = GenericRecord.insert(name, %{"title" => "secret"})

      conn = get(build_conn(), "/api/#{name}/#{record["id"]}")
      refute conn.status == 403
    end

    test "a malformed id is indistinguishable from a missing record" do
      name = public_list_private_view()

      conn = get(build_conn(), "/api/#{name}/not-a-uuid")
      assert conn.status == 404

      assert json_response(conn, 404) ==
               json_response(get(build_conn(), "/api/#{name}/#{Ecto.UUID.generate()}"), 404)
    end
  end

  describe "list denial messages" do
    test "a nil listRule denies with a message that leaks no schema details" do
      name = create_collection(%{"listRule" => nil})

      body = json_response(get(build_conn(), "/api/#{name}"), 403)

      refute body["message"] =~ "SELECT"
      refute body["message"] =~ "Ecto"
      refute body["message"] =~ "Postgrex"
      refute body["message"] =~ "owner_id"
      refute body["message"] =~ "title"
    end
  end

  describe "user-supplied ?filter=" do
    test "an invalid filter returns 400 instead of being silently ignored" do
      name = create_collection(%{"listRule" => ""})
      {:ok, _} = GenericRecord.insert(name, %{"title" => "a"})
      {:ok, _} = GenericRecord.insert(name, %{"title" => "b"})

      # Before the fix, the bad filter was dropped and both rows were returned.
      conn = get(build_conn(), "/api/#{name}", %{filter: "title ="})
      assert json_response(conn, 400)["message"] == "Invalid filter expression."
    end

    test "the rejection message does not echo compiler internals" do
      name = create_collection(%{"listRule" => ""})

      body = json_response(get(build_conn(), "/api/#{name}", %{filter: "'a' ~ 'b'"}), 400)

      refute body["message"] =~ "Unexpected tokens"
      refute body["message"] =~ "does not reference a field"
      refute body["message"] =~ "$1"
    end

    test "a filter naming an unknown field fails closed without a 500" do
      name = create_collection(%{"listRule" => ""})
      {:ok, _} = GenericRecord.insert(name, %{"title" => "visible"})

      conn = get(build_conn(), "/api/#{name}", %{filter: "field_on_another_collection = 'x'"})

      # The unknown column makes the query error; the row helpers swallow query
      # errors, so the caller sees an empty result rather than a stack trace.
      assert conn.status == 200
      assert json_response(conn, 200)["totalItems"] == 0
    end

    test "a valid filter still works" do
      name = create_collection(%{"listRule" => ""})
      {:ok, _} = GenericRecord.insert(name, %{"title" => "keep"})
      {:ok, _} = GenericRecord.insert(name, %{"title" => "drop"})

      body = json_response(get(build_conn(), "/api/#{name}", %{filter: "title = 'keep'"}), 200)
      assert body["totalItems"] == 1
      assert hd(body["items"])["title"] == "keep"
    end
  end
end
