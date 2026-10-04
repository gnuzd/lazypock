defmodule Lazypock.Images.MagickTest do
  @moduledoc """
  `dimensions/1` must work on both ImageMagick 6 and 7.

  IM6 (Debian/Ubuntu, i.e. what CI and most servers install) has `identify` but
  **no** `magick` binary, and `convert identify …` is not a valid IM6 command —
  it makes `convert` treat `identify` as an input file and fail. That silently
  broke the dimension checks (megapixel caps returned `:unknown`, direct uploads
  recorded no width/height) while passing locally on IM7.

  The fake binaries below pin both layouts without needing either version
  installed.
  """
  use ExUnit.Case, async: false

  alias Lazypock.Images.Magick

  @dimensions "120 80"

  setup do
    original_path = System.get_env("PATH")
    on_exit(fn -> System.put_env("PATH", original_path) end)
    :ok
  end

  defp fake_env!(scripts) do
    dir = Path.join(System.tmp_dir!(), "magick-fake-#{System.unique_integer([:positive])}")
    File.mkdir_p!(dir)
    on_exit(fn -> File.rm_rf(dir) end)

    for {name, body} <- scripts do
      path = Path.join(dir, to_string(name))
      File.write!(path, "#!/bin/sh\n#{body}\n")
      File.chmod!(path, 0o755)
    end

    # Hermetic: only the fake binaries are on PATH.
    System.put_env("PATH", dir)
    dir
  end

  test "reads dimensions via the `identify` binary (ImageMagick 6 layout)" do
    fake_env!(
      identify: ~s(echo "#{@dimensions}"),
      # IM6's convert rejects the subcommand — the old code called this.
      convert: "echo \"no decode delegate for this image format \\`identify'\" >&2; exit 1"
    )

    assert {:ok, 120, 80} = Magick.dimensions("/tmp/irrelevant.png")
  end

  test "falls back to `magick identify` when there is no `identify` binary" do
    fake_env!(magick: ~s(if [ "$1" = "identify" ]; then echo "#{@dimensions}"; else exit 1; fi))

    assert {:ok, 120, 80} = Magick.dimensions("/tmp/irrelevant.png")
  end

  test "a failing identify is reported as an error, not a wrong size" do
    fake_env!(identify: "exit 1")

    assert {:error, {:identify_failed, 1}} = Magick.dimensions("/tmp/irrelevant.png")
  end

  test "unparseable output is an error" do
    fake_env!(identify: ~s(echo "not a size"))

    assert {:error, :unknown_dimensions} = Magick.dimensions("/tmp/irrelevant.png")
  end

  test "with no ImageMagick at all it reports :magick_not_found" do
    fake_env!(unrelated: "true")

    assert {:error, :magick_not_found} = Magick.dimensions("/tmp/irrelevant.png")
  end

  describe "with the real ImageMagick" do
    test "reads the dimensions of a real PNG" do
      path = Path.join(System.tmp_dir!(), "magick-real-#{System.unique_integer([:positive])}.png")
      File.write!(path, Lazypock.TestImage.tiny_png!(300, 180))
      on_exit(fn -> File.rm(path) end)

      assert {:ok, 300, 180} = Magick.dimensions(path)
    end
  end
end
