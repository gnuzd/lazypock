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
      |> Lazypock.Collections.Collection.changeset(%{
        rules: Map.merge(default_rules, rules_overrides)
      })
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
      assert {:ok, socket} =
               connect(LazypockWeb.CollectionSocket, %{"token" => superuser_token()})

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

      assert {:error, %{reason: "Access denied"}} =
               subscribe_and_join(socket, "collection:#{name}", %{})
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

  describe "realtime origin-connection exclusion" do
    test "socket connect stores the connection id" do
      {:ok, socket} = connect(LazypockWeb.CollectionSocket, %{"connectionId" => "conn-abc"})
      assert socket.assigns[:connection_id] == "conn-abc"

      {:ok, socket2} = connect(LazypockWeb.CollectionSocket, %{"connection_id" => "conn-def"})
      assert socket2.assigns[:connection_id] == "conn-def"

      {:ok, socket3} = connect(LazypockWeb.CollectionSocket, %{})
      assert socket3.assigns[:connection_id] == nil
    end

    test "a broadcast from the same connection is not delivered" do
      name = cname("chan_self")
      create_collection(name, %{"listRule" => ""})

      {:ok, socket} = connect(LazypockWeb.CollectionSocket, %{"connectionId" => "conn-1"})
      {:ok, _reply, _joined} = subscribe_and_join(socket, "collection:#{name}", %{})

      LazypockWeb.Endpoint.broadcast!("collection:#{name}", "record_change", %{
        "action" => "create",
        "record" => %{"id" => "rec-1", "title" => "hello"},
        "from_connection" => "conn-1"
      })

      refute_receive %Phoenix.Socket.Message{event: "record_change"}
    end

    test "a broadcast from another connection is delivered without the from_connection field" do
      name = cname("chan_other")
      create_collection(name, %{"listRule" => ""})

      {:ok, socket} = connect(LazypockWeb.CollectionSocket, %{"connectionId" => "conn-2"})
      {:ok, _reply, _joined} = subscribe_and_join(socket, "collection:#{name}", %{})

      LazypockWeb.Endpoint.broadcast!("collection:#{name}", "record_change", %{
        "action" => "update",
        "record" => %{"id" => "rec-1", "title" => "hello"},
        "from_connection" => "conn-1"
      })

      assert_push "record_change", %{
        "action" => "update",
        "record" => %{"id" => "rec-1", "title" => "hello"}
      }

      refute_receive %Phoenix.Socket.Message{
        event: "record_change",
        payload: %{"from_connection" => _}
      }
    end

    test "a broadcast without a from_connection is delivered even to a socket with a connection id" do
      name = cname("chan_nofrom")
      create_collection(name, %{"listRule" => ""})

      {:ok, socket} = connect(LazypockWeb.CollectionSocket, %{"connectionId" => "conn-1"})
      {:ok, _reply, _joined} = subscribe_and_join(socket, "collection:#{name}", %{})

      LazypockWeb.Endpoint.broadcast!("collection:#{name}", "record_change", %{
        "action" => "delete",
        "record" => %{"id" => "rec-9"}
      })

      assert_push "record_change", %{"action" => "delete", "record" => %{"id" => "rec-9"}}
    end

    test "clients without a connection id still receive tagged broadcasts (backwards compatible)" do
      name = cname("chan_nocid")
      create_collection(name, %{"listRule" => ""})

      {:ok, socket} = connect(LazypockWeb.CollectionSocket, %{})
      {:ok, _reply, _joined} = subscribe_and_join(socket, "collection:#{name}", %{})

      LazypockWeb.Endpoint.broadcast!("collection:#{name}", "record_change", %{
        "action" => "create",
        "record" => %{"id" => "rec-3"},
        "from_connection" => "other-client"
      })

      assert_push "record_change", %{"action" => "create", "record" => %{"id" => "rec-3"}}
    end
  end

  describe "custom channels" do
    test "anonymous clients can join a custom topic and receive broadcasts" do
      {:ok, socket} = connect(LazypockWeb.CollectionSocket, %{})
      assert {:ok, _reply, joined} = subscribe_and_join(socket, "chat:room1", %{})
      assert joined.assigns[:topic] == "chat:room1"

      LazypockWeb.Endpoint.broadcast!("chat:room1", "new_message", %{"text" => "hi"})
      assert_push "new_message", %{"text" => "hi"}
    end

    test "authenticated clients can join custom topics too" do
      {:ok, socket} = connect(LazypockWeb.CollectionSocket, %{"token" => superuser_token()})
      assert {:ok, _reply, _joined} = subscribe_and_join(socket, "notifications:user1", %{})
    end

    test "custom channel join guards invalid and reserved topics" do
      socket = %Phoenix.Socket{}

      assert {:error, %{reason: "Invalid topic"}} =
               LazypockWeb.CustomChannel.join("", %{}, socket)

      assert {:error, %{reason: "Topic is reserved"}} =
               LazypockWeb.CustomChannel.join("collections", %{}, socket)

      assert {:error, %{reason: "Topic too long"}} =
               LazypockWeb.CustomChannel.join(String.duplicate("a", 300), %{}, socket)

      assert {:ok, _socket} = LazypockWeb.CustomChannel.join("chat:room2", %{}, socket)
    end
  end

  describe "admin channel (collections topic)" do
    test "superuser sockets can join the collections topic" do
      {:ok, socket} = connect(LazypockWeb.CollectionSocket, %{"token" => superuser_token()})
      assert {:ok, _reply, _joined} = subscribe_and_join(socket, "collections", %{})
    end

    test "anonymous sockets are rejected from the collections topic" do
      {:ok, socket} = connect(LazypockWeb.CollectionSocket, %{})

      assert {:error, %{reason: "Authentication required."}} =
               subscribe_and_join(socket, "collections", %{})
    end

    test "non-superuser auth users are rejected from the collections topic" do
      {:ok, socket} = connect(LazypockWeb.CollectionSocket, %{"token" => user_token()})

      assert {:error, %{reason: "Access denied. Superuser required."}} =
               subscribe_and_join(socket, "collections", %{})
    end
  end
end
