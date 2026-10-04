defmodule Lazypock.Files.RulesTest do
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Files.Rules

  # File-rule evaluation shares the collection enforcer's fail-closed
  # semantics; these cover the states that matter for security (who gets in)
  # without spinning up files for every case.

  defp set_rules!(rules) do
    original = Lazypock.Settings.get()
    files = Map.get(original, "files", %{}) || %{}
    Lazypock.Settings.put(Map.put(original, "files", Map.put(files, "rules", rules)))

    on_exit(fn -> Lazypock.Settings.put(original) end)
  end

  defp auth_user(id \\ Ecto.UUID.generate()) do
    %{"id" => id, "email" => "u@test.com", "role" => "user"}
  end

  defp superuser, do: %Lazypock.Auth.SuperUser{id: Ecto.UUID.generate(), email: "s@test.com"}

  test "nil rule denies non-superusers" do
    set_rules!(%{})
    assert {:error, _} = Rules.authorize_list(nil)
    assert {:error, _} = Rules.authorize_list(auth_user())
  end

  test "empty rule is public" do
    set_rules!(%{"listRule" => ""})
    assert {:ok, {"", []}} = Rules.authorize_list(nil)
    assert {:ok, {"", []}} = Rules.authorize_list(auth_user())
  end

  test "whitespace-only rule denies (fail closed)" do
    set_rules!(%{"listRule" => "   "})
    assert {:error, _} = Rules.authorize_list(auth_user())
  end

  test "unknown @request.auth token denies instead of silently matching" do
    set_rules!(%{"listRule" => "uploaded_by = @request.auth.typo"})
    assert {:error, _} = Rules.authorize_list(auth_user())
  end

  test "a valid owner rule compiles to a bound filter" do
    owner = Ecto.UUID.generate()
    set_rules!(%{"listRule" => "uploaded_by = @request.auth.id"})

    assert {:ok, {sql, params}} = Rules.authorize_list(auth_user(owner))
    assert sql =~ "uploaded_by"
    assert params == [owner]
  end

  test "superusers bypass every file rule" do
    set_rules!(%{
      "listRule" => "uploaded_by = 'nobody'",
      "deleteRule" => "uploaded_by = 'nobody'"
    })

    assert {:ok, {"", []}} = Rules.authorize_list(superuser())
    assert :ok = Rules.authorize_delete(superuser(), %{"id" => Ecto.UUID.generate()})
  end

  test "valid_field?/1 recognises file columns" do
    assert Rules.valid_field?("uploaded_by")
    assert Rules.valid_field?("mime_type")
    refute Rules.valid_field?("owner_id")
  end
end
