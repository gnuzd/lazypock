defmodule Lazypock.Images do
  @moduledoc """
  Image engine abstraction.

  Implementations turn an input image into a resized WebP at a destination path:

      Lazypock.Images.engine().resize(source_path, preset_or_geometry, dest_path)

  The engine is selected with `LAZYPOCK_IMAGE_ENGINE`:

    * `magick` (default) — the ImageMagick CLI (`magick`/`convert`), run as an
      isolated child process. Always available in this project.
    * `vix` — libvips via the optional `:vix` dependency. Faster and lighter for
      large JPEG/WebP inputs, but requires libvips on the host and the `:vix`
      dependency; setting this without them raises (unsupported configuration is
      never silently ignored).

  Resizing is subject to `Lazypock.Files.Limiter`, so a burst of requests cannot
  spawn unbounded image processes.
  """

  @type preset :: %{optional(String.t()) => term()}

  @callback resize(String.t(), preset() | String.t(), String.t(), keyword()) ::
              :ok | {:error, term()}

  @callback dimensions(String.t()) :: {:ok, pos_integer(), pos_integer()} | {:error, term()}

  @doc "The configured engine module."
  @spec engine() :: module()
  def engine do
    case System.get_env("LAZYPOCK_IMAGE_ENGINE", "magick") do
      "magick" ->
        Lazypock.Images.Magick

      "vix" ->
        raise ArgumentError,
              "LAZYPOCK_IMAGE_ENGINE=vix is not implemented yet; it requires a " <>
                "Lazypock.Images.Vix module plus the :vix dependency and libvips"

      other ->
        raise ArgumentError,
              "LAZYPOCK_IMAGE_ENGINE=#{inspect(other)} is not supported (use \"magick\")"
    end
  end

  @doc "Whether an image engine is usable right now."
  def available? do
    not is_nil(Lazypock.Images.Magick.find_binary())
  rescue
    _ -> false
  end
end
