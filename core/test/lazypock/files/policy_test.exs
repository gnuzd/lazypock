defmodule Lazypock.Files.PolicyTest do
  use Lazypock.DataCase, async: false

  alias Lazypock.Files.Policy
  alias Lazypock.Settings

  setup do
    original = Settings.get()
    Policy.clear_cache()

    on_exit(fn ->
      Settings.put(original)
      Policy.clear_cache()
    end)

    :ok
  end

  defp put_upload_settings(map) do
    Settings.put(Map.put(Settings.get(), "upload", map))
    Policy.clear_cache()
  end

  describe "resolve/1 precedence" do
    test "falls back to the built-in defaults" do
      policy = Policy.resolve()

      assert Policy.max_size(policy) == 10 * 1024 * 1024
      assert Policy.max_megapixels(policy) == 40
      assert Policy.max_dimension(policy) == 10_000
      assert Policy.mime_allowed?("application/pdf", policy)
    end

    test "global settings override the defaults" do
      put_upload_settings(%{
        "max_size" => 1_048_576,
        "max_megapixels" => 4,
        "mime_types" => ["image/*"]
      })

      policy = Policy.resolve()

      assert Policy.max_size(policy) == 1_048_576
      assert Policy.max_megapixels(policy) == 4
      assert Policy.mime_allowed?("image/png", policy)
      refute Policy.mime_allowed?("application/pdf", policy)
    end

    test "field options win over global settings" do
      put_upload_settings(%{"max_size" => 1_048_576})

      policy =
        Policy.resolve(%{"maxFileSize" => 2_048_576, "mimeTypes" => ["image/png"]})

      assert Policy.max_size(policy) == 2_048_576
      assert Policy.mime_allowed?("image/png", policy)
      refute Policy.mime_allowed?("image/jpeg", policy)
    end

    test "accepts a human-readable size in settings" do
      put_upload_settings(%{"max_size" => "5MB"})
      assert Policy.max_size(Policy.resolve()) == 5 * 1_048_576
    end

    test "accepts snake_case field options too" do
      policy = Policy.resolve(%{"max_size" => 512_000, "mime_types" => ["image/webp"]})

      assert Policy.max_size(policy) == 512_000
      assert Policy.mime_allowed?("image/webp", policy)
    end
  end

  describe "mime_allowed?/2" do
    test "a nil allowlist does not restrict" do
      assert Policy.mime_allowed?("application/pdf", %{"mime_types" => nil})
      assert Policy.mime_allowed?("image/png", %{})
    end

    test "wildcards and exact matches" do
      policy = %{"mime_types" => ["image/*", "application/pdf"]}

      assert Policy.mime_allowed?("image/avif", policy)
      assert Policy.mime_allowed?("application/pdf", policy)
      refute Policy.mime_allowed?("text/plain", policy)
      refute Policy.mime_allowed?(nil, policy)
    end
  end

  describe "parse_size/1" do
    test "parses common units" do
      assert Policy.parse_size("5MB") == 5 * 1_048_576
      assert Policy.parse_size("2mb") == 2 * 1_048_576
      assert Policy.parse_size("1.5kb") == 1536
      assert Policy.parse_size("1gb") == 1_073_741_824
      assert Policy.parse_size("100") == 100
    end

    test "returns nil for nonsense" do
      assert Policy.parse_size("big") == nil
      assert Policy.parse_size("") == nil
    end
  end

  describe "check_image/2" do
    setup do
      path =
        Path.join(System.tmp_dir!(), "lazypock-policy-#{System.unique_integer([:positive])}.png")

      File.write!(path, Lazypock.TestImage.tiny_png!(100, 100))
      on_exit(fn -> File.rm(path) end)
      {:ok, path: path}
    end

    test "passes within the caps", %{path: path} do
      assert :ok = Policy.check_image(path, %{"max_megapixels" => 40, "max_dimension" => 10_000})
    end

    test "rejects an image whose side exceeds max_dimension", %{path: path} do
      assert {:error, :image_too_large} =
               Policy.check_image(path, %{"max_megapixels" => 40, "max_dimension" => 50})
    end

    test "rejects an image over the megapixel cap", %{path: path} do
      assert {:error, :image_too_many_pixels} =
               Policy.check_image(path, %{"max_megapixels" => 0.001, "max_dimension" => 10_000})
    end
  end
end
