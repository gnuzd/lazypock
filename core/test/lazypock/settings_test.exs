defmodule Lazypock.SettingsTest do
  use ExUnit.Case, async: false

  alias Lazypock.Settings

  setup do
    # start a clean slate for each test
    Ecto.Adapters.SQL.Sandbox.checkout(Lazypock.Repo)
    Settings.put(%{})
    on_exit(fn -> Ecto.Adapters.SQL.Sandbox.checkin(Lazypock.Repo) end)
    :ok
  end

  test "new_api_key/0 returns a unique, prefixed key" do
    a = Settings.new_api_key()
    b = Settings.new_api_key()

    assert String.starts_with?(a, "lazypock_")
    assert byte_size(a) > 32
    assert a != b
  end

  test "set_api_key stores only the SHA-256 hash, never the raw key" do
    key = Settings.new_api_key()
    Settings.set_api_key(key)

    # Raw key must not be recoverable from settings.
    refute Settings.get("api_key") == key
    refute String.contains?(Settings.get("api_key", ""), "lazypock_")
    assert Settings.get("api_key") == Settings.hash_key(key)

    # round-trip verification succeeds only for the exact key
    assert Settings.verify_api_key(key)
    refute Settings.verify_api_key(key <> "x")
    refute Settings.verify_api_key("lazypock_wrong")
  end

  test "verify_api_key/nil and unset key return false" do
    assert Settings.verify_api_key(nil) == false
    assert Settings.verify_api_key("lazypock_whatever") == false
  end

  test "has_api_key?/0 reflects configuration" do
    assert Settings.has_api_key?() == false
    Settings.set_api_key(Settings.new_api_key())
    assert Settings.has_api_key?() == true
    Settings.clear_api_key()
    assert Settings.has_api_key?() == false
  end

  test "clear_api_key removes the key" do
    Settings.set_api_key("abc")
    Settings.clear_api_key()
    assert Settings.get("api_key") == nil
    assert Settings.verify_api_key("abc") == false
  end

  test "secure_compare is constant-time equal check" do
    assert Settings.secure_compare("secret", "secret")
    refute Settings.secure_compare("secret", "wrong")
  end

  test "get/put persist a full settings map" do
    Settings.put(%{"app_name" => "My App"})
    assert Settings.get("app_name") == "My App"
    assert Settings.get("missing", "dflt") == "dflt"
  end
end
