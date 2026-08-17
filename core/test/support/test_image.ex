defmodule Lazypock.TestImage do
  @moduledoc """
  Fixture image helpers for tests.

  ImageMagick 7 ships a single `magick` binary, while ImageMagick 6 — the
  version packaged on Ubuntu LTS runners (e.g. GitHub Actions) — only
  provides `convert`/`identify`. Resolve whichever is available, mirroring
  `Lazypock.Files.Adapters.Local.find_magick/0`, so fixture generation works
  on macOS (Homebrew: `magick`) and Ubuntu CI (`convert`) alike.
  """

  @doc "Path to the ImageMagick binary (`magick` or `convert`), or `nil` when absent."
  def magick do
    Enum.find_value(["magick", "convert"], fn cmd -> System.find_executable(cmd) end)
  end

  @doc """
  Generates a small gradient PNG of the given dimensions and returns its binary.

  Raises when ImageMagick is not installed — the thumbnail/scale tests exercise
  real ImageMagick output, so a missing binary is a hard error rather than a
  silent skip.
  """
  def tiny_png!(width, height, colors \\ "navy-orange") do
    case magick() do
      nil ->
        raise """
        ImageMagick not found — the thumbnail/scale tests need it to generate \
        fixture PNGs. Install it (brew install imagemagick / apt install imagemagick).
        """

      bin ->
        path =
          Path.join(
            System.tmp_dir!(),
            "lazypock-test-#{System.unique_integer([:positive])}.png"
          )

        System.cmd(bin, ["-size", "#{width}x#{height}", "gradient:#{colors}", path])
        binary = File.read!(path)
        File.rm(path)
        binary
    end
  end
end
