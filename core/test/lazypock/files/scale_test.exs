defmodule Lazypock.Files.ScaleTest do
  use Lazypock.DataCase, async: true

  alias Lazypock.Files.Store

  # A 300x180 PNG (generated with ImageMagick at test setup time). Resolves
  # `magick` or `convert` (IM7 vs IM6), see Lazypock.TestImage.
  defp tiny_png, do: Lazypock.TestImage.tiny_png!(300, 180)

  # The declared MIME type is not enough: the old implementation wrote an
  # extensionless temp file, so ImageMagick inherited the *input* format (e.g. a
  # PNG/JPEG body) and it was then served as `image/webp`. Assert the container.
  defp webp?(binary) do
    byte_size(binary) >= 12 and binary_part(binary, 0, 4) == "RIFF" and
      binary_part(binary, 8, 4) == "WEBP"
  end

  describe "Store.scale/2" do
    test "scales an image on demand and returns real webp bytes" do
      {:ok, file_record} =
        Store.store(tiny_png(), "demo.png", collection_name: "posts", field_name: "thumbnail")

      {:ok, binary, mime} = Store.scale(file_record, "100x100")
      assert mime == "image/webp"
      assert webp?(binary)

      # Cached: second call returns same bytes
      {:ok, binary2, "image/webp"} = Store.scale(file_record, "100x100")
      assert binary2 == binary
      assert webp?(binary2)

      Store.delete(file_record["id"])
    end

    test "rejects sizes above the dimension cap" do
      {:ok, file_record} =
        Store.store(tiny_png(), "demo.png", collection_name: "posts", field_name: "thumbnail")

      for size <- ["5000", "9999x300", "3000x3000", "x100000"] do
        assert {:error, :invalid_size} = Store.scale(file_record, size),
               "expected #{size} to be rejected"
      end

      # Still accepts the documented forms up to the cap.
      assert {:ok, _, "image/webp"} = Store.scale(file_record, "2000")

      Store.delete(file_record["id"])
    end

    test "rejects invalid sizes" do
      {:ok, file_record} =
        Store.store(tiny_png(), "demo.png", collection_name: "posts", field_name: "thumbnail")

      assert {:error, :invalid_size} = Store.scale(file_record, "99999999")
      assert {:error, :invalid_size} = Store.scale(file_record, "abc")

      Store.delete(file_record["id"])
    end

    test "rejects non-image files" do
      {:ok, file_record} =
        Store.store("hello world", "notes.txt", collection_name: "posts", field_name: "thumbnail")

      assert {:error, :not_an_image} = Store.scale(file_record, "100x100")

      Store.delete(file_record["id"])
    end
  end

  describe "Local.parse_scale_size (via scale)" do
    test "accepts single dimension, box, height, and exact-crop forms" do
      {:ok, file_record} =
        Store.store(tiny_png(), "demo.png", collection_name: "posts", field_name: "thumbnail")

      for size <- ["100", "100x", "x100", "100x200", "100x200!"] do
        assert {:ok, _, "image/webp"} = Store.scale(file_record, size),
               "expected #{size} to scale successfully"
      end

      Store.delete(file_record["id"])
    end
  end

  describe "preset variants" do
    defp uploads_dir do
      Application.get_env(:lazypock, :file_storage)[:path] ||
        Path.join(Application.app_dir(:lazypock, "priv"), "uploads")
    end

    defp variant_dir(file) do
      id = String.replace(file["id"], "-", "")
      Path.join([uploads_dir(), "_variants", id])
    end

    test "eager presets are generated on upload and recorded" do
      {:ok, file} = Store.store(tiny_png(), "demo.png", [])

      assert file["variants"] |> Map.keys() |> Enum.sort() == ["content", "small", "thumb"]
      assert File.exists?(Path.join(variant_dir(file), "thumb.webp"))
      assert File.exists?(Path.join(variant_dir(file), "small.webp"))
      assert File.exists?(Path.join(variant_dir(file), "content.webp"))

      Store.delete(file["id"])
      Lazypock.Files.Reaper.drain()
    end

    test "scale/2 serves a named preset as webp" do
      {:ok, file} = Store.store(tiny_png(), "demo.png", [])

      assert {:ok, binary, "image/webp"} = Store.scale(file, "thumb")
      assert webp?(binary)

      Store.delete(file["id"])
      Lazypock.Files.Reaper.drain()
    end

    test "a cached variant is reused instead of re-rendered" do
      {:ok, file} = Store.store(tiny_png(), "demo.png", [])
      {:ok, first, "image/webp"} = Store.scale(file, "thumb")

      # Replace the original with a differently sized image: a cache hit must
      # still return the first bytes rather than re-rendering.
      {:ok, path} = Lazypock.Files.Adapters.Local.local_path(file)
      File.write!(path, Lazypock.TestImage.tiny_png!(600, 400, "green-blue"))

      assert {:ok, second, "image/webp"} = Store.scale(file, "thumb")
      assert second == first

      Store.delete(file["id"])
      Lazypock.Files.Reaper.drain()
    end

    test "concurrent requests for the same variant agree" do
      {:ok, file} = Store.store(tiny_png(), "demo.png", [])
      # Warm the preset cache so the child tasks never touch the database.
      _ = Lazypock.Files.Presets.all()

      results =
        1..8
        |> Task.async_stream(fn _ -> Store.scale(file, "thumb") end,
          max_concurrency: 8,
          timeout: 30_000
        )
        |> Enum.map(fn {:ok, result} -> result end)

      assert Enum.all?(results, &match?({:ok, _, "image/webp"}, &1))
      assert results |> Enum.map(fn {:ok, b, _} -> b end) |> Enum.uniq() |> length() == 1

      Store.delete(file["id"])
      Lazypock.Files.Reaper.drain()
    end

    test "the reaper removes preset variants" do
      {:ok, file} = Store.store(tiny_png(), "demo.png", [])
      dir = variant_dir(file)
      assert File.exists?(dir)

      Store.delete(file["id"])
      Lazypock.Files.Reaper.drain()

      refute File.exists?(dir)
    end
  end
end
