defmodule Lazypock.Hooks.DisableTest do
  use ExUnit.Case, async: true

  alias Lazypock.Hooks.{User, Registry}

  describe "LAZYPOCK_DISABLE_HOOKS" do
    test "disabled?/0 reflects the env var" do
      old = System.get_env("LAZYPOCK_DISABLE_HOOKS")

      try do
        System.put_env("LAZYPOCK_DISABLE_HOOKS", "1")
        assert User.disabled?()

        System.put_env("LAZYPOCK_DISABLE_HOOKS", "true")
        assert User.disabled?()

        System.put_env("LAZYPOCK_DISABLE_HOOKS", "0")
        refute User.disabled?()

        System.delete_env("LAZYPOCK_DISABLE_HOOKS")
        refute User.disabled?()
      after
        if old, do: System.put_env("LAZYPOCK_DISABLE_HOOKS", old), else: System.delete_env("LAZYPOCK_DISABLE_HOOKS")
      end
    end

    test "load!/0 does not register hooks when disabled" do
      old = System.get_env("LAZYPOCK_DISABLE_HOOKS")

      try do
        System.put_env("LAZYPOCK_DISABLE_HOOKS", "1")
        Registry.clear()

        assert :ok = User.load!()
        assert Registry.all() == []
      after
        if old, do: System.put_env("LAZYPOCK_DISABLE_HOOKS", old), else: System.delete_env("LAZYPOCK_DISABLE_HOOKS")
        Registry.clear()
      end
    end
  end
end
