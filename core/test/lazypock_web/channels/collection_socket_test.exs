defmodule LazypockWeb.CollectionSocketTest do
  use Lazypock.DataCase, async: false

  import Phoenix.ChannelTest

  @endpoint LazypockWeb.Endpoint

  alias Lazypock.Schema.DDL
  alias Lazypock.Collections.Registry
  alias Lazypock.Auth.Token
  alias Lazypock.Auth.SuperUser
  alias Lazypock.Repo

  @moduledoc """
  Realtime channel authorization: socket connect (token validation) and
  channel join (listRule enforcement) for collection subscriptions.
  """

  defp cname(prefix), do: "#{prefix}_#{System.unique_integer([:positive]) |> abs()}"

  defp create_collection(name, rules_overrides) do
    default_rules = %{
      "listRule" => "",
      "viewRule" => "",
      "createRule" => "",
      "updateRule" => "",
      "deleteRule" => "",
      "manageRule" => nil
    }

    {:ok, coll} =
      DDL.create_collection(name,
        type: "base",
        fields: [
          %{"name" => "title", "type" => "text", "required" => false, "indexed" => false},
          %{"name" => "owner_id", "type" => "text", "required" => false, "indexed" => false}
        ]
      )

    {:ok, coll} =
      coll
      |> Lazypock.Collections.Collection.changeset(%{rules: Map.merge(default_rules, rules_overrides)})
      |> Repo.update()

    Registry.reload!()
    coll
  end

  defp superuser_token do
    su = %SuperUser{
      id: Ecto.UUID.generate(),
      email: "admin@test.com",
      password_hash: Bcrypt.hash_pwd_salt("password")
    }

    Repo.insert!(su)
    {:ok, token} = Token.generate_access_token(su)
    token
  end

  defp user_token(overrides \\ %{}) do
    user = %{
      "id" => Map.get(overrides, "id", Ecto.UUID.generate()),
      "email" => Map.get(overrides, "email", "alice@test.com")
    }

    {:ok, token} = Token.generate_user_token(user, Map.get(overrides, "collection", "users"))
    token
  end

  describe "socket connect" do
    test "connects without a token (public access)" do
      assert {:ok, socket} = connect(LazypockWeb.CollectionSocket, %{})
      assert socket.assigns[:current_user] == nil
    end

    test "connects with a superuser token" do
      assert {:ok, socket} = connect(LazypockWeb.CollectionSocket, %{"token" => superuser_token()})
      assert %Lazypock.Auth.SuperUser{} = socket.assigns[:current_user]
    end

    test "connects with an auth collection user token" do
      assert {:ok, socket} = connect(LazypockWeb.CollectionSocket, %{"token" => user_token()})
      assert socket.assigns[:current_user]["id"] != nil
      assert socket.assigns[:current_user]["role"] == "user"
    end

    test "rejects an invalid token" do
      assert {:error, %{reason: "Invalid token"}} =
               connect(LazypockWeb.CollectionSocket, %{"token" => "garbage-token"})
    end
  end

  describe "channel join authorization" do
    test "public collection (empty listRule) is joinable without auth" do
      name = cname("chan_pub")
      create_collection(name, %{"listRule" => ""})

      {:ok, socket} = connect(LazypockWeb.CollectionSocket, %{})
      assert {:ok, _reply, joined} = subscribe_and_join(socket, "collection:#{name}", %{})
      assert joined.assigns[:collection_name] == name
    end

    test "nil listRule denies unauthenticated users" do
      name = cname("chan_deny")
      create_collection(name, %{"listRule" => nil})

      {:ok, socket} = connect(LazypockWeb.CollectionSocket, %{})
      assert {:error, %{reason: "Access denied"}} = subscribe_and_join(socket, "collection:#{name}", %{})
    end

    test "nil listRule allows superusers" do
      name = cname("chan_su")
      create_collection(name, %{"listRule" => nil})

      {:ok, socket} = connect(LazypockWeb.CollectionSocket, %{"token" => superuser_token()})
      assert {:ok, _reply, _joined} = subscribe_and_join(socket, "collection:#{name}", %{})
    end

    test "filtered listRule allows subscription (list semantics: filters apply to the query)" do
      name = cname("chan_filter")
      create_collection(name, %{"listRule" => "@request.auth.role = 'admin'"})

      # Plain user can subscribe — listRule filters the records they see
      {:ok, user_socket} = connect(LazypockWeb.CollectionSocket, %{"token" => user_token()})
      assert {:ok, _reply, _joined} = subscribe_and_join(user_socket, "collection:#{name}", %{})

      # Superuser can subscribe too
      {:ok, su_socket} = connect(LazypockWeb.CollectionSocket, %{"token" => superuser_token()})
      assert {:ok, _reply, _joined} = subscribe_and_join(su_socket, "collection:#{name}", %{})
    end

    test "unknown collection is rejected" do
      {:ok, socket} = connect(LazypockWeb.CollectionSocket, %{"token" => superuser_token()})
      assert {:error, %{reason: "Collection not found"}} =
               subscribe_and_join(socket, "collection:no_such_collection_xyz", %{})
    end

    test "record-scoped topics use the same authorization" do
      name = cname("chan_rec")
      create_collection(name, %{"listRule" => ""})

      {:ok, socket} = connect(LazypockWeb.CollectionSocket, %{})
      assert {:ok, _reply, joined} =
               subscribe_and_join(socket, "collection:#{name}:some-record-id", %{})

      assert joined.assigns[:collection_name] == name
    end

    test "ping returns pong" do
      name = cname("chan_ping")
      create_collection(name, %{"listRule" => ""})

      {:ok, socket} = connect(LazypockWeb.CollectionSocket, %{})
      {:ok, _reply, joined} = subscribe_and_join(socket, "collection:#{name}", %{})
      ref = push(joined, "ping", %{})
      assert_reply ref, :ok, %{ping: "pong"}
    end
  end
end
