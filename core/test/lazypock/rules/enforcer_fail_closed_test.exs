defmodule Lazypock.Rules.EnforcerFailClosedTest do
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Collections.Collection
  alias Lazypock.Collections.Registry
  alias Lazypock.Repo
  alias Lazypock.Rules.Enforcer
  alias Lazypock.Schema.DDL

  # Fail-closed guarantees for the rule enforcer.
  #
  # Every test here encodes the same principle: a rule the enforcer cannot
  # prove it evaluated must DENY. These cover the cases where a rule *value*
  # or *expression* previously produced something the enforcer read as
  # "no restriction".
  #
  #   * whitespace-only rule values (previously treated as the public "")
  #   * non-string rule values
  #   * expressions that took the code generator's catch-all and emitted an
  #     empty clause (e.g. `'a' ~ 'b'`)
  #   * dangling boolean operators (previously silently dropped)
  #   * unknown `@request.auth.*` tokens (previously bound to "")

  @rule_keys ~w(listRule viewRule createRule updateRule deleteRule manageRule)

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
      %{"name" => "owner_id", "type" => "text", "required" => false, "indexed" => false}
    ]

    {:ok, coll} = DDL.create_collection(name, type: "base", fields: fields)

    {:ok, coll} =
      coll
      |> Collection.changeset(%{rules: Map.merge(default_rules, rules_overrides)})
      |> Repo.update()

    Registry.reload!()
    coll
  end

  defp set_rule(collection, rule_key, value) do
    {:ok, updated} =
      collection
      |> Collection.changeset(%{rules: Map.put(collection.rules, rule_key, value)})
      |> Repo.update()

    Registry.reload!()
    updated
  end

  defp superuser, do: %Lazypock.Auth.SuperUser{id: Ecto.UUID.generate(), email: "a@b.com"}

  defp auth_user(overrides \\ %{}) do
    %{
      "id" => Map.get(overrides, "id", Ecto.UUID.generate()),
      "email" => Map.get(overrides, "email", "alice@test.com"),
      "role" => Map.get(overrides, "role", "user")
    }
  end

  defp cname(prefix), do: "#{prefix}_#{System.unique_integer([:positive]) |> abs()}"

  describe "whitespace-only rule values deny (regression: silent public grant)" do
    for rule_key <- @rule_keys do
      test "blank #{rule_key} denies a non-superuser" do
        rule_key = unquote(rule_key)
        name = cname("blank")
        coll = create_test_collection(name, %{rule_key => "   "})

        refute is_nil(coll)

        # authorize_list covers listRule; authorize_mutation covers the rest.
        if rule_key == "listRule" do
          assert {:error, _} = Enforcer.authorize_list(name, auth_user())
        else
          assert {:error, _} =
                   Enforcer.authorize_mutation(name, rule_key, auth_user(), %{
                     "id" => Ecto.UUID.generate(),
                     "title" => "x"
                   })
        end
      end

      test "blank #{rule_key} still denies anonymous" do
        rule_key = unquote(rule_key)
        name = cname("blank_anon")
        create_test_collection(name, %{rule_key => "\t\n  "})

        if rule_key == "listRule" do
          assert {:error, _} = Enforcer.authorize_list(name, nil)
        else
          assert {:error, _} = Enforcer.authorize_mutation(name, rule_key, nil, %{})
        end
      end
    end

    test "a blank rule still allows a superuser (bypass is checked first)" do
      name = cname("blank_su")
      create_test_collection(name, %{"listRule" => "   "})

      assert {:ok, _} = Enforcer.authorize_list(name, superuser())
    end

    test "a blank manageRule does not grant delegated management" do
      name = cname("blank_manage")
      create_test_collection(name, %{"listRule" => nil, "manageRule" => "   "})

      assert {:error, _} = Enforcer.authorize_list(name, auth_user())
      assert {:error, _} = Enforcer.authorize_manage(name, auth_user())
    end

    test "an empty-string manageRule is public delegation (documented)" do
      name = cname("public_manage")
      create_test_collection(name, %{"listRule" => nil, "manageRule" => ""})

      assert :ok = Enforcer.authorize_manage(name, auth_user())
    end
  end

  describe "non-string rule values deny" do
    test "a numeric listRule denies instead of crashing the compiler" do
      name = cname("numeric_rule")
      coll = create_test_collection(name, %{"listRule" => ""})
      set_rule(coll, "listRule", 123)

      assert {:error, _} = Enforcer.authorize_list(name, auth_user())
    end

    test "a numeric updateRule denies" do
      name = cname("numeric_update")
      coll = create_test_collection(name, %{"updateRule" => ""})
      set_rule(coll, "updateRule", 42)

      assert {:error, _} =
               Enforcer.authorize_update(name, auth_user(), %{"id" => Ecto.UUID.generate()})
    end
  end

  describe "expressions that cannot produce SQL deny (regression: empty-clause allow)" do
    # These used to compile to {:ok, {"", []}}, which the enforcer read as
    # "no restriction" and therefore allowed unconditionally.

    test "a rule whose left operand is not a field denies" do
      name = cname("no_field")
      create_test_collection(name, %{"listRule" => "'a' ~ 'b'"})

      assert {:error, _} = Enforcer.authorize_list(name, auth_user())
      assert {:error, _} = Enforcer.authorize_list(name, nil)
    end

    test "an array-operator rule with no field denies" do
      name = cname("no_field_arr")
      create_test_collection(name, %{"listRule" => "'a' ?= 'b'"})

      assert {:error, _} = Enforcer.authorize_list(name, auth_user())
    end

    test "a mutation rule with no field denies" do
      name = cname("no_field_mut")
      create_test_collection(name, %{"viewRule" => "$1 ~ 'b'"})

      assert {:error, _} =
               Enforcer.authorize_view(name, auth_user(), %{"id" => Ecto.UUID.generate()})
    end

    test "a dangling boolean operator denies instead of silently dropping it" do
      name = cname("dangling")
      create_test_collection(name, %{"listRule" => "title = 'x' &&"})

      assert {:error, _} = Enforcer.authorize_list(name, auth_user())
    end

    test "a dangling || denies" do
      name = cname("dangling_or")
      create_test_collection(name, %{"listRule" => "title = 'x' ||"})

      assert {:error, _} = Enforcer.authorize_list(name, auth_user())
    end

    test "an over-long rule denies" do
      name = cname("long_rule")
      create_test_collection(name, %{"listRule" => String.duplicate("title = 'x' || ", 400)})

      assert {:error, _} = Enforcer.authorize_list(name, auth_user())
    end
  end

  describe "@request.auth.* unknown tokens deny (regression: empty-string grant)" do
    test "an unknown token is rejected rather than bound to empty string" do
      name = cname("unknown_token")

      # Before the fix this resolved to `'' = ''` -> true -> allow.
      create_test_collection(name, %{"listRule" => "@request.auth.typo = ''"})

      assert {:error, _} = Enforcer.authorize_list(name, auth_user())
      assert {:error, _} = Enforcer.authorize_list(name, nil)
    end

    test "an unknown token on a mutation rule denies" do
      name = cname("unknown_token_mut")
      create_test_collection(name, %{"updateRule" => "@request.auth.nope = ''"})

      assert {:error, _} =
               Enforcer.authorize_update(name, auth_user(), %{"id" => Ecto.UUID.generate()})
    end

    test "unknowable nested relation tokens deny" do
      name = cname("nested_token")
      create_test_collection(name, %{"listRule" => "@request.auth.relation.id = 'x'"})

      assert {:error, _} = Enforcer.authorize_list(name, auth_user(%{"id" => "u1"}))
    end

    test "the supported tokens still work" do
      name = cname("known_tokens")
      create_test_collection(name, %{"listRule" => "owner_id = @request.auth.id"})

      user = auth_user(%{"id" => "user-42"})
      assert {:ok, {sql, params}} = Enforcer.authorize_list(name, user)
      assert sql =~ "owner_id"
      assert params == ["user-42"]
    end

    test "role token still works with its default" do
      name = cname("role_token")
      create_test_collection(name, %{"listRule" => "@request.auth.role = 'admin'"})

      # `role` resolves to a bound param compared against the rule literal; a
      # default-role user therefore does not match and is denied at eval time.
      assert {:ok, {"$1 = $2", ["user", "admin"]}} = Enforcer.authorize_list(name, auth_user())

      assert {:ok, {"$1 = $2", ["admin", "admin"]}} =
               Enforcer.authorize_list(name, auth_user(%{"role" => "admin"}))
    end
  end

  describe "superuser detection is not structural (regression: any struct bypassed rules)" do
    test "only a real SuperUser struct bypasses a nil rule" do
      name = cname("struct_bypass")
      create_test_collection(name, %{"listRule" => nil})

      assert {:ok, _} = Enforcer.authorize_list(name, superuser())
    end

    test "an unrelated struct does NOT get the superuser bypass" do
      name = cname("struct_denied")
      create_test_collection(name, %{"listRule" => nil, "viewRule" => nil})

      # An unrelated struct used to satisfy `%{__struct__: _}`.
      for impostor <- [%URI{}, %Collection{name: "x"}] do
        assert {:error, _} = Enforcer.authorize_list(name, impostor)
        assert {:error, _} = Enforcer.authorize_view(name, impostor, %{"id" => "x"})
      end
    end
  end

  describe "documented parity semantics" do
    test "a guest's @request.auth.id is the empty string (PocketBase parity)" do
      # Documented, not silently accepted: a guest's id token binds to "",
      # so `!=` comparisons against it evaluate against the empty string.
      # The realistic guest guard (`@request.auth.id != ''`) correctly denies
      # guests, which is covered in enforcer_test.exs.
      name = cname("guest_empty")
      create_test_collection(name, %{"listRule" => "title != @request.auth.id"})

      assert {:ok, {sql, [""]}} = Enforcer.authorize_list(name, nil)
      assert sql =~ "title"
    end
  end
end
