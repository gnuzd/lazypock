defmodule Lazypock.Files.ValidationTest do
  use ExUnit.Case, async: true

  alias Lazypock.Files.Validation

  @png <<0x89, ?P, ?N, ?G, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D>>
  @jpeg <<0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10>>
  @gif "GIF89a" <> <<0x01, 0x00, 0x01, 0x00, 0x00>>
  @webp "RIFF" <> <<0x04, 0x00, 0x00, 0x00>> <> "WEBP"
  @avif <<0x00, 0x00, 0x00, 0x20>> <> "ftypavif" <> :binary.copy(<<0>>, 16)
  @pdf "%PDF-1.7\n1 0 obj\n"
  @zip <<0x50, 0x4B, 0x03, 0x04, 0x14, 0x00>>
  @mp4 <<0x00, 0x00, 0x00, 0x18>> <> "ftypisom"
  @mp3 "ID3" <> <<0x03, 0x00, 0x00, 0x00>>

  describe "validate/2 accepts genuine files" do
    test "images" do
      assert {:ok, "image/png"} = Validation.validate("a.png", @png)
      assert {:ok, "image/jpeg"} = Validation.validate("a.jpg", @jpeg)
      assert {:ok, "image/jpeg"} = Validation.validate("a.JPEG", @jpeg)
      assert {:ok, "image/gif"} = Validation.validate("a.gif", @gif)
      assert {:ok, "image/webp"} = Validation.validate("a.webp", @webp)
      assert {:ok, "image/avif"} = Validation.validate("a.avif", @avif)
    end

    test "documents and archives" do
      assert {:ok, "application/pdf"} = Validation.validate("a.pdf", @pdf)
      assert {:ok, "application/zip"} = Validation.validate("a.zip", @zip)
      assert {:ok, "video/mp4"} = Validation.validate("a.mp4", @mp4)
      assert {:ok, "audio/mpeg"} = Validation.validate("a.mp3", @mp3)
    end

    test "text formats" do
      assert {:ok, "text/plain"} = Validation.validate("a.txt", "hello world")
      assert {:ok, "text/csv"} = Validation.validate("a.csv", "a,b\n1,2\n")
      assert {:ok, "application/json"} = Validation.validate("a.json", ~s({"ok":true}))
    end
  end

  describe "validate/2 rejects disguised or disallowed files" do
    test "extension not on the allowlist" do
      assert {:error, :extension_not_allowed} = Validation.validate("shell.php", "<?php ?>")
      assert {:error, :extension_not_allowed} = Validation.validate("evil.sh", "#!/bin/sh\n")
      assert {:error, :extension_not_allowed} = Validation.validate("active.svg", "<svg/>")
      assert {:error, :extension_not_allowed} = Validation.validate("noext", "hello")
    end

    test "content does not match the extension" do
      # A script masquerading as an image.
      assert {:error, :content_mismatch} = Validation.validate("a.png", "<?php echo 1; ?>")
      # A PNG renamed to a PDF.
      assert {:error, :content_mismatch} = Validation.validate("a.pdf", @png)
      # MP4 renamed to PNG.
      assert {:error, :content_mismatch} = Validation.validate("a.png", @mp4)
    end

    test "text extension with binary content" do
      assert {:error, :invalid_text} = Validation.validate("a.txt", <<0x00, 0xFF, 0xFE>>)
      assert {:error, :invalid_text} = Validation.validate("a.json", "{not json")
    end
  end

  describe "extension/1 and sniff/1" do
    test "downcases and isolates the extension" do
      assert Validation.extension("Photo.PNG") == ".png"
      assert Validation.extension("archive.tar.gz") == ".gz"
      # Path.extname/1 never returns a path separator, so traversal is moot.
      assert Validation.extension("a.png/../../etc/passwd") == ""
    end

    test "sniff/1 reports :unknown for text" do
      assert Validation.sniff("just text") == :unknown
      assert {:ok, "image/png"} = Validation.sniff(@png)
    end
  end

  describe "validate_file/2" do
    setup do
      dir = Path.join(System.tmp_dir!(), "lazypock-vf-#{System.unique_integer([:positive])}")
      File.mkdir_p!(dir)
      on_exit(fn -> File.rm_rf(dir) end)
      %{dir: dir}
    end

    defp write(dir, name, bytes) do
      path = Path.join(dir, name)
      File.write!(path, bytes)
      path
    end

    test "accepts an image read straight from disk", %{dir: dir} do
      path = write(dir, "a.png", @png <> :binary.copy(<<0>>, 200))
      assert {:ok, "image/png"} = Validation.validate_file("a.png", path)
    end

    test "rejects a mismatch and a disallowed extension without a full read", %{dir: dir} do
      assert {:error, :content_mismatch} =
               Validation.validate_file("a.png", write(dir, "a.png", "<?php ?>"))

      assert {:error, :extension_not_allowed} =
               Validation.validate_file("a.svg", write(dir, "a.svg", "<svg/>"))
    end

    test "still fully reads text files for the UTF-8/JSON checks", %{dir: dir} do
      assert {:ok, "application/json"} =
               Validation.validate_file("a.json", write(dir, "a.json", ~s({"ok":true})))

      assert {:error, :invalid_text} =
               Validation.validate_file("a.json", write(dir, "bad.json", "{not json"))
    end

    test "returns an error for a missing file", %{dir: dir} do
      assert {:error, :enoent} = Validation.validate_file("a.png", Path.join(dir, "nope.png"))
    end
  end
end
