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

  test "create_api_key/0 returns a raw key and metadata, persists only the hash" do
    {key, meta} = Settings.create_api_key()
    assert String.starts_with?(key, "lazypock_")
    assert meta["id"] && is_binary(meta["id"])
    assert meta["revoked"] == false
    assert meta["expires_at"] == nil

    # Raw key must not be recoverable from settings.
    stored = Settings.list_api_keys()
    assert length(stored) == 1
    refute stored == [%{"hash" => key}]

    # Verification succeeds only for the exact raw key.
    assert Settings.verify_api_key(key)
    refute Settings.verify_api_key(key <> "x")
    refute Settings.verify_api_key("lazypock_wrong")
  end

  test "create_api_key/1 sets an expiry when days > 0" do
    {key, meta} = Settings.create_api_key(7)
    assert meta["expires_at"] != nil
    assert Settings.verify_api_key(key)

    # Date is ~7 days in the future.
    future = DateTime.from_iso8601(meta["expires_at"])
    assert {:ok, future_dt, _} = future
    assert DateTime.compare(future_dt, DateTime.utc_now()) == :gt
  end

  test "verify_api_key/nil and unset key return false" do
    assert Settings.verify_api_key(nil) == false
    assert Settings.verify_api_key("lazypock_whatever") == false
  end

  test "has_api_key?/0 reflects configuration" do
    assert Settings.has_api_key?() == false
    Settings.create_api_key()
    assert Settings.has_api_key?() == true
  end

  test "revoke_api_key marks a key revoked (keeps it in the list)" do
    {key, meta} = Settings.create_api_key()
    assert Settings.revoke_api_key(meta["id"]) == :ok
    assert Settings.verify_api_key(key) == false
    assert Settings.has_api_key?() == false

    [item] = Settings.list_api_keys()
    assert item["revoked"] == true
  end

  test "revoke_api_key/unknown id returns :error" do
    assert Settings.revoke_api_key("does-not-exist") == :error
  end

  test "delete_api_key! removes a key entirely" do
    {_key1, meta1} = Settings.create_api_key()
    {key2, _meta2} = Settings.create_api_key()

    assert Settings.delete_api_key!(meta1["id"]) == :ok
    assert length(Settings.list_api_keys()) == 1
    assert Settings.verify_api_key(key2)
  end

  test "expired keys are rejected" do
    {key, meta} = Settings.create_api_key()
    # Force expiry into the past by rewriting stored metadata.
    now = DateTime.utc_now()
    past = now |> DateTime.add(-3600, :second) |> DateTime.to_iso8601()

    Settings.put(%{
      "api_keys" => [
        %{
          "id" => meta["id"],
          "hash" => Settings.hash_key(key),
          "created_at" => DateTime.to_iso8601(now),
          "expires_at" => past,
          "revoked" => false
        }
      ]
    })

    assert Settings.verify_api_key(key) == false
    assert Settings.has_api_key?() == false
  end

  test "multiple keys can coexist and verify independently" do
    {key1, _} = Settings.create_api_key(7)
    {key2, _} = Settings.create_api_key()
    assert length(Settings.list_api_keys()) == 2

    assert Settings.verify_api_key(key1)
    assert Settings.verify_api_key(key2)

    # Revoking one leaves the other active.
    revoke_id = hd(Settings.list_api_keys())["id"]
    :ok = Settings.revoke_api_key(revoke_id)

    assert length(Settings.list_api_keys()) == 2
    # Exactly one is revoked.
    assert Enum.count(Settings.list_api_keys(), & &1["revoked"]) == 1
  end

  test "legacy single-key format still verifies (back-compat)" do
    legacy_hash = Settings.hash_key("legacy_lazypock_secret")
    Settings.put(%{"api_key" => legacy_hash})

    assert Settings.verify_api_key("legacy_lazypock_secret")
    refute Settings.verify_api_key("wrong")
    assert Settings.has_api_key?() == true
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
