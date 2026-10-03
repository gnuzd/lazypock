defmodule Lazypock.Files.StorageTest do
  use Lazypock.DataCase, async: false

  alias Lazypock.Files.Storage
  alias Lazypock.Files.Storage.Secret
  alias Lazypock.Settings

  @env_vars ~w(LAZYPOCK_S3_ENDPOINT LAZYPOCK_S3_REGION LAZYPOCK_S3_BUCKET LAZYPOCK_S3_ACCESS_KEY LAZYPOCK_S3_SECRET LAZYPOCK_S3_PUBLIC_URL LAZYPOCK_S3_PREFIX)

  setup do
    original = Settings.get()
    Storage.clear_cache()

    on_exit(fn ->
      for var <- @env_vars, do: System.delete_env(var)
      Settings.put(original)
      Storage.clear_cache()
    end)

    :ok
  end

  defp put_storage(map) do
    Settings.put(Map.put(Settings.get(), "storage", map))
    Storage.clear_cache()
  end

  test "defaults to the local backend" do
    assert Storage.backend() == "local"
    refute Storage.s3?()
    assert Storage.validate(%{"backend" => "local"}) == :ok
  end

  test "settings select the s3 backend and normalise prefix/endpoint" do
    put_storage(%{
      "backend" => "s3",
      "endpoint" => "https://account.r2.cloudflarestorage.com/",
      "bucket" => "media",
      "access_key_id" => "key",
      "secret_access_key" => "secret",
      "prefix" => "lazypock/app"
    })

    config = Storage.config()
    assert Storage.s3?()
    assert config["endpoint"] == "https://account.r2.cloudflarestorage.com"
    assert config["prefix"] == "lazypock/app/"
  end

  test "environment variables win over settings" do
    put_storage(%{"backend" => "s3", "bucket" => "from-settings"})
    System.put_env("LAZYPOCK_S3_BUCKET", "from-env")
    Storage.clear_cache()

    assert Storage.config()["bucket"] == "from-env"
    assert "bucket" in Storage.configured_from_env()
    assert Storage.env_locked?("bucket")
  end

  test "validates the required s3 fields" do
    assert {:error, _} = Storage.validate(%{"backend" => "s3", "bucket" => "b"})
    assert {:error, _} = Storage.validate(%{"backend" => "wat"})

    assert :ok =
             Storage.validate(%{
               "backend" => "s3",
               "endpoint" => "https://e",
               "bucket" => "b",
               "access_key_id" => "k",
               "secret_access_key" => "s"
             })
  end

  test "merge encrypts a new secret and preserves the stored one when masked" do
    put_storage(%{"backend" => "s3", "bucket" => "b"})
    stored = Settings.get("storage")

    merged = Storage.merge(stored, %{"secret_access_key" => "super-secret"})
    assert String.starts_with?(merged["secret_access_key"], "enc:")
    assert {:ok, "super-secret"} = Secret.decrypt(merged["secret_access_key"])

    put_storage(merged)
    assert Storage.config()["secret_access_key"] == "super-secret"

    # A masked/blank update keeps the previous secret.
    again = Storage.merge(Settings.get("storage"), %{"secret_access_key" => Storage.mask()})
    assert again["secret_access_key"] == merged["secret_access_key"]
  end

  test "public_view never exposes the secret" do
    put_storage(%{
      "backend" => "s3",
      "bucket" => "b",
      "secret_access_key" => "super-secret"
    })

    view = Storage.public_view()
    assert view["secret_access_key"] == Storage.mask()
    assert view["secret_set"] == true
    refute inspect(view) =~ "super-secret"
  end
end
