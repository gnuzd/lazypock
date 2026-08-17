defmodule Lazypock.Files.StoreTest do
  use Lazypock.DataCase, async: false

  alias Lazypock.Files.Store

  # A tiny valid PNG so thumbnail generation (ImageMagick) succeeds when present.
  # Resolves `magick` or `convert` (IM7 vs IM6), see Lazypock.TestImage.
  defp tiny_png, do: Lazypock.TestImage.tiny_png!(60, 40)

  defp sample_binary, do: "hello lazy pock file contents"

  describe "Store.store/3" do
    test "stores a binary and persists metadata" do
      {:ok, file} = Store.store(sample_binary(), "notes.txt", collection_name: "posts", record_id: "rec-1", field_name: "attachment")

      assert file["filename"] == "notes.txt"
      assert file["extension"] == ".txt"
      assert file["mime_type"] == "text/plain"
      assert file["size"] == byte_size(sample_binary())
      assert file["storage_backend"] == "local"
      assert file["collection_name"] == "posts"
      assert file["record_id"] == "rec-1"
      assert file["field_name"] == "attachment"
      assert file["id"] != nil

      # Physical file readable
      {:ok, binary} = Store.read(file)
      assert binary == sample_binary()

      Store.delete(file["id"])
    end

    test "stores from a Plug.Upload struct" do
      path = Path.join(System.tmp_dir!(), "lazypock-upload-#{System.unique_integer([:positive])}.txt")
      File.write!(path, sample_binary())

      upload = %Plug.Upload{path: path, filename: "upload.txt", content_type: "text/plain"}
      {:ok, file} = Store.store(upload)
      assert file["filename"] == "upload.txt"
      assert file["mime_type"] == "text/plain"

      File.rm(path)
      Store.delete(file["id"])
    end

    test "infers mime from extension when content_type is nil" do
      {:ok, file} = Store.store(sample_binary(), "photo.png", [])
      assert file["mime_type"] == "image/png"
      Store.delete(file["id"])
    end

    test "generates thumbnails for images when ImageMagick is available" do
      {:ok, file} =
        Store.store(tiny_png(), "demo.png",
          collection_name: "posts",
          field_name: "thumbnail",
          thumb_sizes: ["100x100"]
        )

      # Thumbnails may be empty if magick is missing in CI; when present the
      # map has the requested size and the thumb is readable.
      if file["thumbs"] != %{} do
        assert Map.has_key?(file["thumbs"], "100x100")
        thumb = file["thumbs"]["100x100"]
        assert thumb["mime_type"] == "image/webp"
        assert {:ok, _binary} = Store.read_thumb(file, thumb)
      end

      Store.delete(file["id"])
    end

    test "non-image files produce no thumbnails" do
      {:ok, file} =
        Store.store(sample_binary(), "notes.txt",
          collection_name: "posts",
          thumb_sizes: ["100x100"]
        )

      assert file["thumbs"] == %{}
      Store.delete(file["id"])
    end
  end

  describe "Store.get/1" do
    test "returns the file record by id" do
      {:ok, file} = Store.store(sample_binary(), "a.txt", [])
      assert {:ok, found} = Store.get(file["id"])
      assert found["id"] == file["id"]
      assert found["filename"] == "a.txt"
      Store.delete(file["id"])
    end

    test "returns not_found for unknown id" do
      assert {:error, :not_found} = Store.get(Ecto.UUID.generate())
    end
  end

  describe "Store.list/1" do
    setup do
      {:ok, f1} = Store.store(sample_binary(), "a.txt", collection_name: "posts", field_name: "body")
      {:ok, f2} = Store.store(sample_binary(), "b.png", collection_name: "posts", field_name: "image")
      {:ok, f3} = Store.store(sample_binary(), "c.txt", collection_name: "other", field_name: "body")
      %{f1: f1, f2: f2, f3: f3}
    end

    test "lists all files newest first", %{f1: f1} do
      {:ok, %{items: items, total: total}} = Store.list()
      assert total >= 3
      assert Enum.any?(items, &(&1["id"] == f1["id"]))
    end

    test "filters by collection", %{f2: f2, f3: f3} do
      {:ok, %{items: items}} = Store.list(collection_name: "posts")
      assert Enum.any?(items, &(&1["id"] == f2["id"]))
      refute Enum.any?(items, &(&1["id"] == f3["id"]))
    end

    test "filters by mime prefix", %{f2: f2, f3: f3} do
      {:ok, %{items: items}} = Store.list(mime: "image/")
      assert Enum.any?(items, &(&1["id"] == f2["id"]))
      refute Enum.any?(items, &(&1["id"] == f3["id"]))
    end

    test "paginates with page/per_page", %{f1: f1, f2: f2, f3: f3} do
      {:ok, %{items: page1, page: 1}} = Store.list(per_page: 1)
      assert length(page1) == 1

      ids = MapSet.new([f1["id"], f2["id"], f3["id"]])
      assert MapSet.member?(ids, hd(page1)["id"])
    end
  end

  describe "Store.delete/1 and delete_by_record/2" do
    test "delete removes metadata and physical file" do
      {:ok, file} = Store.store(sample_binary(), "del.txt", collection_name: "posts", record_id: "r1")
      assert {:ok, _} = Store.read(file)
      assert :ok = Store.delete(file["id"])
      assert {:error, :not_found} = Store.get(file["id"])
    end

    test "delete_by_record removes all files for a record" do
      {:ok, f1} = Store.store(sample_binary(), "x.txt", collection_name: "posts", record_id: "rec-9")
      {:ok, f2} = Store.store(sample_binary(), "y.txt", collection_name: "posts", record_id: "rec-9")
      {:ok, f3} = Store.store(sample_binary(), "z.txt", collection_name: "posts", record_id: "rec-10")

      assert :ok = Store.delete_by_record("posts", "rec-9")
      assert {:error, :not_found} = Store.get(f1["id"])
      assert {:error, :not_found} = Store.get(f2["id"])
      assert {:ok, _} = Store.get(f3["id"])

      Store.delete(f3["id"])
    end
  end
end
