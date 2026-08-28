defmodule Lazypock.Rules.EnforcerGapsTest do
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Rules.Enforcer
  alias Lazypock.Schema.DDL
  alias Lazypock.Schema.TypeMapper
  alias Lazypock.Schemas.GenericRecord
  alias Lazypock.Collections.Registry

  # Tests for enforcer paths not covered by enforcer_test.exs:
  #   * manageRule short-circuit on mutations (view/create/update/delete)
  #   * action-rule nil + manageRule match interplay
  #   * invalid filter rules → denied without crashing
  #   * rules referencing records that don't exist (or were deleted) → denied
  #   * quote-escaping of @request.auth.* values

  defp create_test_collection(name, rules_overrides) do
    default_rules = %{
      "listRule" => "",
      "viewRule" => "",
      "createRule" => "",
      "updateRule" => "",
      "deleteRule" => "",
      "manageRule" => nil
    }

    fields = [
      %{"name" => "title", "type" => "text", "required" => false, "indexed" => false},
      %{"name" => "owner_id", "type" => "text", "required" => false, "indexed" => false},
      %{"name" => "status", "type" => "text", "required" => false, "indexed" => false}
    ]

    rules = Map.merge(default_rules, rules_overrides)

    {:ok, coll} = DDL.create_collection(name, type: "base", fields: fields)

    {:ok, coll} =
      coll
      |> Lazypock.Collections.Collection.changeset(%{rules: rules})
      |> Lazypock.Repo.update()

    Registry.reload!()
    coll
  end

  defp insert_record(collection_name, attrs) do
    {:ok, record} = GenericRecord.insert(collection_name, attrs)
    record
  end

  defp superuser do
    %Lazypock.Auth.SuperUser{id: Ecto.UUID.generate(), email: "admin@test.com"}
  end

  defp auth_user(overrides \\ %{}) do
    %{
      "id" => Map.get(overrides, "id", Ecto.UUID.generate()),
      "email" => Map.get(overrides, "email", "alice@test.com"),
      "role" => Map.get(overrides, "role", "user")
    }
  end

  defp cname(prefix), do: "#{prefix}_#{System.unique_integer([:positive]) |> abs()}"

  describe "manageRule short-circuit on mutations" do
    test "manageRule match allows view when viewRule is nil" do
      name = cname("mgmt_view")
      create_test_collection(name, %{"viewRule" => nil, "manageRule" => "@request.auth.role = 'admin'"})
      record = insert_record(name, %{title: "x"})
      assert :ok = Enforcer.authorize_view(name, auth_user(%{"role" => "admin"}), record)
      assert {:error, _} = Enforcer.authorize_view(name, auth_user(%{"role" => "user"}), record)
    end

    test "manageRule match allows create when createRule is nil" do
      name = cname("mgmt_create")
      create_test_collection(name, %{"createRule" => nil, "manageRule" => "title = 'new'"})
      admin = auth_user()
      assert :ok = Enforcer.authorize_create(name, admin, %{"title" => "new"})
      assert {:error, _} = Enforcer.authorize_create(name, admin, %{"title" => "other"})
    end

    test "manageRule match allows update when updateRule is nil" do
      name = cname("mgmt_update")
      create_test_collection(name, %{"updateRule" => nil, "manageRule" => "@request.auth.role = 'editor'"})
      record = insert_record(name, %{title: "x"})
      editor = auth_user(%{"role" => "editor"})
      assert :ok = Enforcer.authorize_update(name, editor, record)
      assert {:error, _} = Enforcer.authorize_update(name, auth_user(%{"role" => "viewer"}), record)
    end

    test "manageRule match allows delete when deleteRule is nil" do
      name = cname("mgmt_delete")
      create_test_collection(name, %{"deleteRule" => nil, "manageRule" => "@request.auth.id = 'root'"})
      record = insert_record(name, %{title: "x"})
      assert :ok = Enforcer.authorize_delete(name, auth_user(%{"id" => "root"}), record)
      assert {:error, _} = Enforcer.authorize_delete(name, auth_user(%{"id" => "other"}), record)
    end

    test "manageRule evaluated against the record (record-field rule)" do
      name = cname("mgmt_record")
      create_test_collection(name, %{"updateRule" => nil, "manageRule" => "owner_id = @request.auth.id"})
      record = insert_record(name, %{owner_id: "user-1"})
      owner = auth_user(%{"id" => "user-1"})
      other = auth_user(%{"id" => "user-2"})
      assert :ok = Enforcer.authorize_update(name, owner, record)
      assert {:error, _} = Enforcer.authorize_update(name, other, record)
    end
  end

  describe "invalid rules" do
    test "a filter that fails to compile denies instead of crashing" do
      name = cname("badfilter")
      create_test_collection(name, %{"listRule" => "((("})
      user = auth_user()

      assert {:error, _} = Enforcer.authorize_list(name, user)
    end

    test "a rule referencing an unknown field denies" do
      name = cname("unknownfield")
      create_test_collection(name, %{"viewRule" => "no_such_field = 'x'"})
      record = insert_record(name, %{title: "hello"})
      assert {:error, _} = Enforcer.authorize_view(name, auth_user(), record)
    end

    test "listRule that fails to compile denies list access" do
      name = cname("badlist")
      create_test_collection(name, %{"listRule" => "title ="})
      assert {:error, _} = Enforcer.authorize_list(name, auth_user())
    end
  end

  describe "record existence" do
    test "view rule referencing a record that no longer exists is denied" do
      name = cname("gone")
      create_test_collection(name, %{"viewRule" => "owner_id = @request.auth.id"})
      record = insert_record(name, %{owner_id: "user-1"})
      owner = auth_user(%{"id" => "user-1"})
      assert :ok = Enforcer.authorize_view(name, owner, record)

      # Delete the underlying row — the rule eval now finds nothing
      GenericRecord.delete(name, record["id"])
      assert {:error, _} = Enforcer.authorize_view(name, owner, record)
    end
  end

  describe "SQL injection / escaping" do
    test "@request.auth.email with a quote is escaped safely" do
      name = cname("sqlesc")

      create_test_collection(name, %{
        "viewRule" => "@request.auth.email = 'admin@x.com' OR '1'='1'"
      })

      # A malicious email that would break out of quotes must NOT match
      attacker = auth_user(%{"email" => "x' OR '1'='1"})
      record = insert_record(name, %{title: "secret"})
      assert {:error, _} = Enforcer.authorize_view(name, attacker, record)
    end

    test "legitimate matching email still works after escaping" do
      name = cname("escok")

      create_test_collection(name, %{"viewRule" => "@request.auth.email = 'o''brien@x.com'"})

      record = insert_record(name, %{title: "x"})
      user = auth_user(%{"email" => "o'brien@x.com"})
      assert :ok = Enforcer.authorize_view(name, user, record)
    end

    test "rule value containing a single quote matches only the exact record" do
      name = cname("quoteval")

      create_test_collection(name, %{
        "viewRule" => "owner_id = @request.auth.id",
        "createRule" => "owner_id = @request.auth.id"
      })

      record = insert_record(name, %{title: "x", owner_id: "O'Brien-1"})
      owner = auth_user(%{"id" => "O'Brien-1"})
      stranger = auth_user(%{"id" => "Smith-2"})

      assert :ok = Enforcer.authorize_view(name, owner, record)
      assert {:error, _} = Enforcer.authorize_view(name, stranger, record)

      # Same value through the create path (no record id yet)
      assert :ok = Enforcer.authorize_create(name, owner, %{"owner_id" => "O'Brien-1"})
      assert {:error, _} = Enforcer.authorize_create(name, owner, %{"owner_id" => "other"})
    end

    test "rule value containing a backslash is bound safely" do
      name = cname("backslash")
      create_test_collection(name, %{"viewRule" => "owner_id = @request.auth.id"})

      record = insert_record(name, %{title: "x", owner_id: "user\\1"})
      owner = auth_user(%{"id" => "user\\1"})
      stranger = auth_user(%{"id" => "user\\2"})

      assert :ok = Enforcer.authorize_view(name, owner, record)
      assert {:error, _} = Enforcer.authorize_view(name, stranger, record)

      # A trailing backslash is data, not an escape
      record2 = insert_record(name, %{title: "y", owner_id: "C:\\path\\"})
      assert :ok = Enforcer.authorize_view(name, auth_user(%{"id" => "C:\\path\\"}), record2)
    end
  end

  describe "typed column casts" do
    defp create_collection_with_fields(name, fields, rules) do
      {:ok, coll} = DDL.create_collection(name, type: "base", fields: fields)

      {:ok, coll} =
        coll
        |> Lazypock.Collections.Collection.changeset(%{rules: rules})
        |> Lazypock.Repo.update()

      Registry.reload!()
      coll
    end

    test "cast for a non-uuid column comes from TypeMapper, not hardcoded" do
      name = cname("cast_fields")

      create_collection_with_fields(
        name,
        [
          %{"name" => "score", "type" => "number", "required" => false, "indexed" => false},
          %{"name" => "title", "type" => "text", "required" => false, "indexed" => false}
        ],
        %{"listRule" => "score > '5' && title != ''"}
      )

      {:ok, {sql, params}} = Enforcer.authorize_list(name, auth_user())

      # Casts are derived from the collection's field types via TypeMapper
      assert sql =~ "\"score\" > $1::#{TypeMapper.to_pg_with_opts("number", %{})}"
      assert sql =~ "\"title\" != $2::#{TypeMapper.to_pg_with_opts("text", %{})}"
      # ... not a hardcoded uuid cast on a non-uuid column
      refute sql =~ "UUID"
      assert is_list(params)
    end

    test "numeric-column rule with TypeMapper cast evaluates against a record" do
      name = cname("cast_num_rec")

      create_collection_with_fields(
        name,
        [%{"name" => "score", "type" => "number", "required" => false, "indexed" => false}],
        %{"viewRule" => "score > '5'"}
      )

      high = insert_record(name, %{score: 10})
      low = insert_record(name, %{score: 3})
      user = auth_user()

      assert :ok = Enforcer.authorize_view(name, user, high)
      assert {:error, _} = Enforcer.authorize_view(name, user, low)
    end

    test "text-column rule with TypeMapper cast evaluates against a record" do
      name = cname("cast_text_rec")
      create_test_collection(name, %{"viewRule" => "owner_id = @request.auth.id"})

      record = insert_record(name, %{title: "x", owner_id: "user-1"})
      owner = auth_user(%{"id" => "user-1"})
      stranger = auth_user(%{"id" => "user-2"})

      assert :ok = Enforcer.authorize_view(name, owner, record)
      assert {:error, _} = Enforcer.authorize_view(name, stranger, record)
    end
  end

  describe "superuser bypass on mutations with nil rules" do
    test "superuser bypasses nil rules for view/create/update/delete" do
      name = cname("subypass")
      create_test_collection(name, %{"viewRule" => nil, "createRule" => nil, "updateRule" => nil, "deleteRule" => nil})
      record = insert_record(name, %{title: "x"})
      su = superuser()

      assert :ok = Enforcer.authorize_view(name, su, record)
      assert :ok = Enforcer.authorize_create(name, su, %{title: "y"})
      assert :ok = Enforcer.authorize_update(name, su, record)
      assert :ok = Enforcer.authorize_delete(name, su, record)
    end
  end
end
