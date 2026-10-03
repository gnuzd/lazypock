defmodule Lazypock.Images.Magick do
  @moduledoc """
  ImageMagick (`magick` / `convert`) implementation of `Lazypock.Images`.

  ImageMagick runs as an isolated child process, so a malformed image can crash
  the CLI without taking down the BEAM. Every invocation is bounded by
  `-limit memory/map` (see `LAZYPOCK_MAGICK_MEMORY_LIMIT`) and metadata is
  stripped, `-auto-orient` applied, and output forced to WebP.

  Concurrency is bounded by `Lazypock.Files.Limiter`, not here.
  """

  @behaviour Lazypock.Images

  require Logger

  @default_quality 80
  @default_memory "256MiB"

  @doc "Path to the ImageMagick binary (`magick` or `convert`), or `nil`."
  def find_binary do
    Enum.find_value(["magick", "convert"], fn cmd -> System.find_executable(cmd) end)
  end

  @impl true
  def resize(source, geometry_or_preset, dest, opts \\ []) do
    case find_binary() do
      nil ->
        {:error, :magick_not_found}

      magick ->
        quality =
          Keyword.get(opts, :quality) || preset_quality(geometry_or_preset) || @default_quality

        run(magick, source, ops(geometry_or_preset), dest, quality)
    end
  end

  @impl true
  def dimensions(path) do
    case find_binary() do
      nil ->
        {:error, :magick_not_found}

      magick ->
        args = [
          "identify",
          "-limit",
          "memory",
          "128MiB",
          "-limit",
          "map",
          "256MiB",
          "-format",
          "%w %h",
          path
        ]

        case System.cmd(magick, args, stderr_to_stdout: true) do
          {out, 0} ->
            case Regex.run(~r/^(\d+)\s+(\d+)$/, String.trim(out)) do
              [_, w, h] -> {:ok, String.to_integer(w), String.to_integer(h)}
              _ -> {:error, :unknown_dimensions}
            end

          {_out, code} ->
            {:error, {:identify_failed, code}}
        end
    end
  rescue
    e -> {:error, e}
  end

  # ── Geometry ─────────────────────────────────────────

  defp ops(%{"width" => w} = preset) when is_integer(w) do
    height = preset["height"]
    fit = preset["fit"] || "contain"

    cond do
      is_integer(height) and fit == "cover" ->
        ["-resize", "#{w}x#{height}^", "-gravity", "center", "-extent", "#{w}x#{height}"]

      is_integer(height) and fit == "exact" ->
        ["-resize", "#{w}x#{height}!"]

      is_integer(height) ->
        # `>` = shrink to fit, never upscale.
        ["-resize", "#{w}x#{height}>"]

      true ->
        ["-resize", "#{w}>"]
    end
  end

  defp ops(geometry) when is_binary(geometry), do: ["-resize", geometry]
  defp ops(_), do: []

  defp preset_quality(%{"quality" => q}) when is_integer(q), do: q
  defp preset_quality(_), do: nil

  # ── Execution ────────────────────────────────────────

  defp run(magick, source, ops, dest, quality) do
    File.mkdir_p!(Path.dirname(dest))

    args =
      [
        "-limit",
        "memory",
        memory_limit(),
        "-limit",
        "map",
        memory_limit(),
        source,
        "-auto-orient",
        "-strip"
      ] ++ ops ++ ["-quality", to_string(quality), "webp:#{dest}"]

    case System.cmd(magick, args, stderr_to_stdout: true) do
      {_out, 0} ->
        :ok

      {out, code} ->
        Logger.debug("ImageMagick resize failed (#{code}): #{String.trim(out)}")
        {:error, :resize_failed}
    end
  rescue
    e -> {:error, e}
  end

  defp memory_limit do
    System.get_env("LAZYPOCK_MAGICK_MEMORY_LIMIT") || @default_memory
  end
end
