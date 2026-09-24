defmodule LazypockWeb.CollectionDeleteTest do
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Schema.DDL
  alias Lazypock.Repo
  alias Lazypock.Auth.SuperUser
  alias Lazypock.Auth.Token
  alias Lazypock.Collections.Registry

  import Ecto.Query

  defp auth_conn(conn) do
    superuser = %SuperUser{
      id: Ecto.UUID.generate(),
      email: "del_#{System.unique_integer([:positive])}@test.com",
      password_hash: Bcrypt.hash_pwd_salt("password")
    }

    Repo.insert!(superuser)
    {:ok, token} = Token.generate_access_token(superuser)
    put_req_header(conn, "authorization", "Bearer #{token}")
  end

  defp unique_name(prefix), do: "#{prefix}_#{:erlang.unique_integer([:positive])}"

  test "DELETE /api/collections/:id deletes a base collection by id" do
    name = unique_name("delme")
    {:ok, coll} = DDL.create_collection(name, type: "base", fields: [])
    Registry.reload!()

    conn = delete(auth_conn(build_conn()), "/api/collections/#{coll.id}")
    assert response(conn, 204)

    Registry.reload!()
    assert Registry.get(name) == {:error, :not_found}
  end

  test "DELETE /api/collections/:name deletes by name too" do
    name = unique_name("delbyname")
    {:ok, _coll} = DDL.create_collection(name, type: "base", fields: [])
    Registry.reload!()

    conn = delete(auth_conn(build_conn()), "/api/collections/#{name}")
    assert response(conn, 204)

    Registry.reload!()
    assert Registry.get(name) == {:error, :not_found}
  end

  test "deletes a collection that has a relation pointing at it from elsewhere" do
    target = unique_name("deltarget")
    source = unique_name("delsource")

    {:ok, _} = DDL.create_collection(target, type: "base", fields: [])
    Registry.reload!()

    {:ok, _} =
      DDL.create_collection(source,
        type: "base",
        fields: [%{"name" => "ref", "type" => "relation", "options" => %{"collection" => target}}]
      )

    Registry.reload!()

    conn = delete(auth_conn(build_conn()), "/api/collections/#{target}")
    assert response(conn, 204)
  end

  test "refuses to delete a system collection" do
    conn = delete(auth_conn(build_conn()), "/api/collections/_superusers")
    assert json_response(conn, 400)["error"] =~ "system collection"
  end

  test "deletes an unmanaged collection (metadata only, no 400)" do
    name = unique_name("delunmanaged")
    {:ok, _coll} = DDL.create_collection(name, type: "base", fields: [])

    Repo.update_all(
      from(c in Lazypock.Collections.Collection, where: c.name == ^name),
      set: [managed: false]
    )

    Registry.reload!()

    conn = delete(auth_conn(build_conn()), "/api/collections/#{name}")
    assert response(conn, 204)

    Registry.reload!()
    assert Registry.get(name) == {:error, :not_found}
  end
end
