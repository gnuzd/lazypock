defmodule Lazypock.Files.ScaleTest do
  use Lazypock.DataCase, async: true

  alias Lazypock.Files.Store

  # A 300x180 PNG (generated with ImageMagick at test setup time).
  defp tiny_png do
    path = Path.join(System.tmp_dir!(), "lazypock-test-scale-#{System.unique_integer([:positive])}.png")
    System.cmd("magick", ["-size", "300x180", "gradient:navy-orange", path])
    binary = File.read!(path)
    File.rm(path)
    binary
  end

  describe "Store.scale/2" do
    test "scales an image on demand and returns webp" do
      {:ok, file_record} =
        Store.store(tiny_png(), "demo.png", collection_name: "posts", field_name: "thumbnail")

      {:ok, binary, mime} = Store.scale(file_record, "100x100")
      assert mime == "image/webp"
      assert byte_size(binary) > 0

      # Cached: second call returns same bytes
      {:ok, binary2, "image/webp"} = Store.scale(file_record, "100x100")
      assert binary2 == binary

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
end
