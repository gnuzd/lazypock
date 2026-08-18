defmodule Lazypock.Rules.EnforcerTest do
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Rules.Enforcer
  alias Lazypock.Schema.DDL
  alias Lazypock.Schemas.GenericRecord
  alias Lazypock.Collections.Registry

  @moduledoc """
  Tests for Lazypock.Rules.Enforcer — access control rule evaluation.

  These tests exercise the full rule pipeline:
    1. Superuser bypass (Ecto structs skip all rules)
    2. manageRule (admin-like delegated access)
    3. Three-state rule logic (nil → deny, "" → allow, filter → conditional)
    4. @request.auth.* token resolution (id, email, role)
    5. Record-level rules (view, update, delete) evaluated via SQL subquery
    6. Attr-level rules (create) evaluated via field substitution
    7. Cross-collection isolation
  """

  # ── Test helpers ─────────────────────────────────────

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

    # Create the collection with DDL (which sets its own default rules)
    {:ok, coll} = DDL.create_collection(name, type: "base", fields: fields)

    # Update the rules via Ecto changeset so :map serializes as JSONB
    {:ok, coll} =
      coll
      |> Lazypock.Collections.Collection.changeset(%{rules: rules})
      |> Lazypock.Repo.update()

    # Reload registry so it picks up the new rules
    Registry.reload!()

    coll
  end

  defp insert_record(collection_name, attrs) do
    {:ok, record} = GenericRecord.insert(collection_name, attrs)
    record
  end

  defp superuser do
    %Lazypock.Auth.SuperUser{
      id: Ecto.UUID.generate(),
      email: "admin@test.com"
    }
  end

  defp auth_user(overrides \\ %{}) do
    %{
      "id" => Map.get(overrides, "id", Ecto.UUID.generate()),
      "email" => Map.get(overrides, "email", "alice@test.com"),
      "role" => Map.get(overrides, "role", "user")
    }
  end

  # ── Superuser bypass behavior ───────────────────────────

  describe "superuser bypass" do
    test "superuser bypasses nil listRule" do
      create_test_collection("su_bypass_list", %{"listRule" => nil})
      assert {:ok, _} = Enforcer.authorize_list("su_bypass_list", superuser())
    end

    test "superuser bypasses nil viewRule" do
      create_test_collection("su_bypass_view", %{"viewRule" => nil})
      record = insert_record("su_bypass_view", %{title: "Secret"})
      assert :ok = Enforcer.authorize_view("su_bypass_view", superuser(), record)
    end

    test "superuser bypasses nil createRule" do
      create_test_collection("su_bypass_create", %{"createRule" => nil})
      assert :ok = Enforcer.authorize_create("su_bypass_create", superuser(), %{title: "New"})
    end

    test "superuser bypasses nil updateRule" do
      create_test_collection("su_bypass_update", %{"updateRule" => nil})
      record = insert_record("su_bypass_update", %{title: "Old"})
      assert :ok = Enforcer.authorize_update("su_bypass_update", superuser(), record)
    end

    test "superuser bypasses nil deleteRule" do
      create_test_collection("su_bypass_delete", %{"deleteRule" => nil})
      record = insert_record("su_bypass_delete", %{title: "DeleteMe"})
      assert :ok = Enforcer.authorize_delete("su_bypass_delete", superuser(), record)
    end

    test "superuser bypasses nil manageRule" do
      create_test_collection("su_bypass_manage", %{"manageRule" => nil})
      assert :ok = Enforcer.authorize_manage("su_bypass_manage", superuser())
    end

    test "superuser bypass is collection-independent" do
      create_test_collection("su_bypass_a", %{"listRule" => nil})
      create_test_collection("su_bypass_b", %{"listRule" => nil})
      assert {:ok, _} = Enforcer.authorize_list("su_bypass_a", superuser())
      assert {:ok, _} = Enforcer.authorize_list("su_bypass_b", superuser())
    end

    test "plain map (auth user) does NOT get superuser bypass" do
      create_test_collection("su_no_bypass", %{"listRule" => nil})
      assert {:error, _} = Enforcer.authorize_list("su_no_bypass", auth_user())
    end
  end

  # ── listRule ──────────────────────────────────────────

  describe "authorize_list/2" do
    test "listRule: nil denies non-superuser" do
      create_test_collection("list_nil_test", %{"listRule" => nil})
      assert {:error, _} = Enforcer.authorize_list("list_nil_test", auth_user())
    end

    test "listRule: nil allows superuser" do
      create_test_collection("list_nil_su", %{"listRule" => nil})
      assert {:ok, _} = Enforcer.authorize_list("list_nil_su", superuser())
    end

    test "listRule: empty string allows anyone" do
      create_test_collection("list_empty_test", %{"listRule" => ""})
      assert {:ok, _} = Enforcer.authorize_list("list_empty_test", auth_user())
      assert {:ok, _} = Enforcer.authorize_list("list_empty_test", superuser())
      assert {:ok, _} = Enforcer.authorize_list("list_empty_test", nil)
    end

    test "listRule: filter matching @request.auth.id returns SQL clause" do
      create_test_collection("list_filter_test", %{
        "listRule" => "owner_id = @request.auth.id"
      })
      user = auth_user(%{"id" => "user-123"})
      assert {:ok, {sql, params}} = Enforcer.authorize_list("list_filter_test", user)
      assert is_binary(sql) and sql != ""
      assert is_list(params)
    end

    test "listRule: filter with @request.auth.role matched" do
      create_test_collection("list_role_test", %{
        "listRule" => "@request.auth.role = 'admin'"
      })
      user = auth_user(%{"role" => "admin"})
      assert {:ok, {sql, _params}} = Enforcer.authorize_list("list_role_test", user)
      # The resolved rule should produce a SQL clause that can be evaluated
      assert is_binary(sql) and sql != ""
    end

    test "manageRule allows access even when listRule is nil" do
      create_test_collection("manage_list_test", %{
        "listRule" => nil,
        "manageRule" => "@request.auth.role = 'admin'"
      })
      admin = auth_user(%{"role" => "admin"})
      assert {:ok, _} = Enforcer.authorize_list("manage_list_test", admin)
    end

    test "manageRule does not grant access when user does not match" do
      create_test_collection("manage_deny_test", %{
        "listRule" => nil,
        "manageRule" => "@request.auth.role = 'admin'"
      })
      user = auth_user(%{"role" => "user"})
      assert {:error, _} = Enforcer.authorize_list("manage_deny_test", user)
    end

    test "unauthenticated user sees public collections only" do
      create_test_collection("list_public_test", %{"listRule" => ""})
      create_test_collection("list_private_test", %{"listRule" => nil})
      assert {:ok, _} = Enforcer.authorize_list("list_public_test", nil)
      assert {:error, _} = Enforcer.authorize_list("list_private_test", nil)
    end
  end

  # ── viewRule ──────────────────────────────────────────

  describe "authorize_view/3" do
    test "viewRule: nil denies non-superuser" do
      create_test_collection("view_nil_test", %{"viewRule" => nil})
      record = insert_record("view_nil_test", %{title: "Secret"})
      assert {:error, _} = Enforcer.authorize_view("view_nil_test", auth_user(), record)
    end

    test "viewRule: nil allows superuser" do
      create_test_collection("view_nil_su", %{"viewRule" => nil})
      record = insert_record("view_nil_su", %{title: "Secret"})
      assert :ok = Enforcer.authorize_view("view_nil_su", superuser(), record)
    end

    test "viewRule: empty string allows anyone" do
      create_test_collection("view_empty_test", %{"viewRule" => ""})
      record = insert_record("view_empty_test", %{title: "Public"})
      assert :ok = Enforcer.authorize_view("view_empty_test", auth_user(), record)
      assert :ok = Enforcer.authorize_view("view_empty_test", nil, record)
    end

    test "viewRule: filter matched against record" do
      create_test_collection("view_filter_test", %{
        "viewRule" => "owner_id = @request.auth.id"
      })
      record = insert_record("view_filter_test", %{title: "Mine", owner_id: "user-1"})
      owner = auth_user(%{"id" => "user-1"})
      stranger = auth_user(%{"id" => "user-2"})
      assert :ok = Enforcer.authorize_view("view_filter_test", owner, record)
      assert {:error, _} = Enforcer.authorize_view("view_filter_test", stranger, record)
    end
  end

  # ── createRule ────────────────────────────────────────

  describe "authorize_create/3" do
    test "createRule: nil denies non-superuser" do
      create_test_collection("create_nil_test", %{"createRule" => nil})
      attrs = %{title: "New"}
      assert {:error, _} = Enforcer.authorize_create("create_nil_test", auth_user(), attrs)
    end

    test "createRule: nil allows superuser" do
      create_test_collection("create_nil_su", %{"createRule" => nil})
      attrs = %{title: "New"}
      assert :ok = Enforcer.authorize_create("create_nil_su", superuser(), attrs)
    end

    test "createRule: empty string allows anyone" do
      create_test_collection("create_empty_test", %{"createRule" => ""})
      assert :ok = Enforcer.authorize_create("create_empty_test", auth_user(), %{title: "New"})
      assert :ok = Enforcer.authorize_create("create_empty_test", nil, %{title: "New"})
    end

    test "createRule: filter evaluated against attrs" do
      create_test_collection("create_filter_test", %{
        "createRule" => "@request.auth.id != ''"
      })
      assert :ok = Enforcer.authorize_create("create_filter_test", auth_user(%{"id" => "u1"}), %{})
      assert {:error, _} = Enforcer.authorize_create("create_filter_test", nil, %{})
    end

    test "createRule: filter matching record field values" do
      create_test_collection("create_field_test", %{
        "createRule" => "status = 'draft'"
      })
      assert :ok = Enforcer.authorize_create("create_field_test", auth_user(), %{status: "draft"})
      assert {:error, _} = Enforcer.authorize_create("create_field_test", auth_user(), %{status: "published"})
    end
  end

  # ── updateRule ────────────────────────────────────────

  describe "authorize_update/4" do
    test "updateRule: nil denies non-superuser" do
      create_test_collection("update_nil_test", %{"updateRule" => nil})
      record = insert_record("update_nil_test", %{title: "Old"})
      assert {:error, _} = Enforcer.authorize_update("update_nil_test", auth_user(), record)
    end

    test "updateRule: nil allows superuser" do
      create_test_collection("update_nil_su", %{"updateRule" => nil})
      record = insert_record("update_nil_su", %{title: "Old"})
      assert :ok = Enforcer.authorize_update("update_nil_su", superuser(), record)
    end

    test "updateRule: empty string allows anyone" do
      create_test_collection("update_empty_test", %{"updateRule" => ""})
      record = insert_record("update_empty_test", %{title: "Editable"})
      assert :ok = Enforcer.authorize_update("update_empty_test", auth_user(), record)
    end

    test "updateRule: filter matched against existing record" do
      create_test_collection("update_filter_test", %{
        "updateRule" => "owner_id = @request.auth.id"
      })
      record = insert_record("update_filter_test", %{title: "Mine", owner_id: "user-1"})
      owner = auth_user(%{"id" => "user-1"})
      stranger = auth_user(%{"id" => "user-2"})
      assert :ok = Enforcer.authorize_update("update_filter_test", owner, record)
      assert {:error, _} = Enforcer.authorize_update("update_filter_test", stranger, record)
    end
  end

  # ── deleteRule ────────────────────────────────────────

  describe "authorize_delete/3" do
    test "deleteRule: nil denies non-superuser" do
      create_test_collection("delete_nil_test", %{"deleteRule" => nil})
      record = insert_record("delete_nil_test", %{title: "ToDelete"})
      assert {:error, _} = Enforcer.authorize_delete("delete_nil_test", auth_user(), record)
    end

    test "deleteRule: nil allows superuser" do
      create_test_collection("delete_nil_su", %{"deleteRule" => nil})
      record = insert_record("delete_nil_su", %{title: "ToDelete"})
      assert :ok = Enforcer.authorize_delete("delete_nil_su", superuser(), record)
    end

    test "deleteRule: empty string allows anyone" do
      create_test_collection("delete_empty_test", %{"deleteRule" => ""})
      record = insert_record("delete_empty_test", %{title: "Deletable"})
      assert :ok = Enforcer.authorize_delete("delete_empty_test", auth_user(), record)
    end

    test "deleteRule: filter matched against record" do
      create_test_collection("delete_filter_test", %{
        "deleteRule" => "owner_id = @request.auth.id"
      })
      record = insert_record("delete_filter_test", %{title: "Mine", owner_id: "user-1"})
      owner = auth_user(%{"id" => "user-1"})
      stranger = auth_user(%{"id" => "user-2"})
      assert :ok = Enforcer.authorize_delete("delete_filter_test", owner, record)
      assert {:error, _} = Enforcer.authorize_delete("delete_filter_test", stranger, record)
    end
  end

  # ── manageRule ───────────────────────────────────────

  describe "authorize_manage/2" do
    test "manageRule: nil denies non-superuser" do
      create_test_collection("manage_nil_test", %{"manageRule" => nil})
      assert {:error, _} = Enforcer.authorize_manage("manage_nil_test", auth_user())
    end

    test "manageRule: nil allows superuser" do
      create_test_collection("manage_nil_su_test", %{"manageRule" => nil})
      assert :ok = Enforcer.authorize_manage("manage_nil_su_test", superuser())
    end

    test "manageRule: matched user gets access" do
      create_test_collection("manage_match_test", %{
        "manageRule" => "@request.auth.role = 'admin'"
      })
      admin = auth_user(%{"role" => "admin"})
      assert :ok = Enforcer.authorize_manage("manage_match_test", admin)
    end

    test "manageRule: non-matched user denied" do
      create_test_collection("manage_nonmatch_test", %{
        "manageRule" => "@request.auth.role = 'admin'"
      })
      user = auth_user(%{"role" => "user"})
      assert {:error, _} = Enforcer.authorize_manage("manage_nonmatch_test", user)
    end
  end

  # ── Cross-collection isolation ─────────────────────────

  describe "cross-collection isolation" do
    test "rules for one collection don't affect another" do
      create_test_collection("strict_a", %{"listRule" => nil})
      create_test_collection("open_b", %{"listRule" => ""})

      user = auth_user()
      assert {:error, _} = Enforcer.authorize_list("strict_a", user)
      assert {:ok, _} = Enforcer.authorize_list("open_b", user)
    end
  end

  # ── Unauthenticated (nil user) ────────────────────────

  describe "unauthenticated requests" do
    test "nil user is denied for nil rules" do
      create_test_collection("unauth_nil", %{"listRule" => nil})
      assert {:error, _} = Enforcer.authorize_list("unauth_nil", nil)
    end

    test "nil user is allowed for empty rules" do
      create_test_collection("unauth_empty", %{"listRule" => ""})
      assert {:ok, _} = Enforcer.authorize_list("unauth_empty", nil)
    end

    test "nil user cannot match @request.auth.id filters" do
      create_test_collection("unauth_filter", %{
        "listRule" => "@request.auth.id != ''"
      })
      assert {:ok, {sql, _}} = Enforcer.authorize_list("unauth_filter", nil)
      # The compiled SQL should resolve to '' != '' which is false
      assert sql != ""
    end
  end

  # ── @request.auth.* resolution ───────────────────────

  describe "@request.auth.* token resolution" do
    test "authenticated user can match against their own id" do
      create_test_collection("token_id_test", %{
        "viewRule" => "owner_id = @request.auth.id"
      })
      record = insert_record("token_id_test", %{title: "Mine", owner_id: "user-99"})
      owner = auth_user(%{"id" => "user-99"})
      assert :ok = Enforcer.authorize_view("token_id_test", owner, record)
    end

    test "authenticated user can match against their email" do
      create_test_collection("token_email_test", %{
        "viewRule" => "@request.auth.email = 'alice@test.com'"
      })
      record = insert_record("token_email_test", %{title: "Alice's record"})
      user = auth_user(%{"email" => "alice@test.com"})
      assert :ok = Enforcer.authorize_view("token_email_test", user, record)
    end

    test "role-based access control works" do
      create_test_collection("token_role_test", %{
        "viewRule" => "@request.auth.role = 'admin'"
      })
      record = insert_record("token_role_test", %{title: "Admin only"})
      admin = auth_user(%{"role" => "admin"})
      user = auth_user(%{"role" => "user"})
      assert :ok = Enforcer.authorize_view("token_role_test", admin, record)
      assert {:error, _} = Enforcer.authorize_view("token_role_test", user, record)
    end

    test "multi-condition rule with auth tokens (regression: #38)" do
      # https://github.com/gnuzd/lazypock/issues/38 — a rule combining an
      # @request.auth.* comparison with another condition. After token
      # resolution the auth comparison becomes literal-vs-literal ($1 = $2),
      # and as the second clause it used to emit "$2 = $2" — silently allowing
      # the wrong-role owner (and crashing the param list on other paths).
      create_test_collection("token_multi_test", %{
        "viewRule" => "owner_id = @request.auth.id && @request.auth.role = 'admin'"
      })
      record = insert_record("token_multi_test", %{title: "Owned by admin", owner_id: "user-99"})

      # Owner with the right role → allowed
      admin_owner = auth_user(%{"id" => "user-99", "role" => "admin"})
      assert :ok = Enforcer.authorize_view("token_multi_test", admin_owner, record)

      # Same owner but wrong role → must be DENIED (buggy code allowed this)
      wrong_role = auth_user(%{"id" => "user-99", "role" => "user"})
      assert {:error, _} = Enforcer.authorize_view("token_multi_test", wrong_role, record)

      # Admin role but different owner → denied
      other_owner = auth_user(%{"id" => "user-55", "role" => "admin"})
      assert {:error, _} = Enforcer.authorize_view("token_multi_test", other_owner, record)
    end

    test "multi-role OR rule with auth tokens (regression: #38)" do
      # Two literal-vs-literal clauses after token resolution. Buggy output was
      # "$1 = $2 OR $3 = $2" — the second clause's right-hand side re-bound to
      # the first clause's value, so a 'board' user was wrongly denied.
      create_test_collection("token_multirole_test", %{
        "viewRule" => "@request.auth.role = 'admin' || @request.auth.role = 'board'"
      })
      record = insert_record("token_multirole_test", %{title: "Role gated"})

      assert :ok =
               Enforcer.authorize_view(
                 "token_multirole_test",
                 auth_user(%{"role" => "admin"}),
                 record
               )

      assert :ok =
               Enforcer.authorize_view(
                 "token_multirole_test",
                 auth_user(%{"role" => "board"}),
                 record
               )

      assert {:error, _} =
               Enforcer.authorize_view(
                 "token_multirole_test",
                 auth_user(%{"role" => "user"}),
                 record
               )
    end
  end
end
