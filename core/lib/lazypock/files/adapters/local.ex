defmodule Lazypock.Files.Adapters.Local do
  @moduledoc """
  Local filesystem adapter.

  Stores files in `priv/uploads/` organized by date:
    priv/uploads/YYYY/MM/DD/{uuid}.{ext}

  Thumbnail generation uses ImageMagick (`magick` or `convert`). If it is not
  installed, uploads still work but no thumbnails are generated (a warning is
  logged once). Set the env var `LAZYPOCK_THUMBNAILS=0` to disable generation
  entirely.
  """

  require Logger

  @behaviour Lazypock.Files.Adapter

  @image_exts ~w(.jpg .jpeg .png .gif .webp)

  @doc """
  Stores a file on the local filesystem.
  """
  @impl true
  def store(binary, filename, _opts) do
    ext = Path.extname(filename)
    uuid = Ecto.UUID.generate()
    date_path = date_based_path()
    storage_path = Path.join([base_path(), date_path, "#{uuid}#{ext}"])
    dir = Path.dirname(storage_path)
    File.mkdir_p!(dir)

    case File.write(storage_path, binary) do
      :ok ->
        {:ok,
         %{
           path: Path.join(date_path, "#{uuid}#{ext}"),
           size: byte_size(binary),
           mime_type: mime_type(ext)
         }}

      {:error, reason} ->
        {:error, reason}
    end
  end

  @impl true
  def url(file_record) do
    "/api/files/#{file_record["id"]}"
  end

  @impl true
  def get(file_record) do
    full_path = Path.join(base_path(), file_record["storage_path"])

    case File.read(full_path) do
      {:ok, binary} -> {:ok, binary}
      {:error, reason} -> {:error, reason}
    end
  end

  @impl true
  def delete(file_record) do
    full_path = Path.join(base_path(), file_record["storage_path"])
    File.rm(full_path)

    # Remove generated thumbnails too
    file_record
    |> thumb_paths()
    |> Enum.each(fn p -> File.rm(Path.join(base_path(), p)) end)

    # Try to clean up empty parent dirs (ignore errors)
    clean_empty_dirs(Path.dirname(full_path))
    :ok
  end

  @impl true
  def thumbs(binary, filename, sizes) do
    cond do
      not image?(filename) ->
        {:ok, []}

      thumbnails_disabled?() ->
        {:ok, []}

      true ->
        case parse_sizes(sizes) do
          {:error, _} ->
            {:ok, []}

          {:ok, geometry} ->
            case find_magick() do
              {:ok, magick} ->
                generate_with_magick(magick, binary, filename, geometry)

              :error ->
                warn_missing_magick()
                {:ok, []}
            end
        end
    end
  end

  defp generate_with_magick(magick, binary, _filename, geometry) do
    tmp_in = temp_path("lazypock-in")
    tmp_dir = Path.dirname(tmp_in)
    File.write!(tmp_in, binary)

    results =
      geometry
      |> Enum.map(fn {size, geom} -> make_thumb(magick, tmp_in, tmp_dir, size, geom) end)
      |> Enum.reject(&is_nil/1)

    File.rm(tmp_in)
    {:ok, results}
  end

  # Logged once per VM so users notice thumbnails are silently skipped.
  @warning_sent_key {__MODULE__, :missing_magick_warned}

  defp warn_missing_magick do
    if not :persistent_term.get(@warning_sent_key, false) do
      :persistent_term.put(@warning_sent_key, true)

      Logger.warning(
        "ImageMagick not found — thumbnail generation disabled. " <>
          "Install it (brew install imagemagick / apt install imagemagick) or set " <>
          "LAZYPOCK_THUMBNAILS=0 to silence this warning."
      )
    end
  end

  # Set LAZYPOCK_THUMBNAILS=0 to disable thumbnail generation entirely.
  defp thumbnails_disabled? do
    System.get_env("LAZYPOCK_THUMBNAILS") == "0"
  end

  def thumb_get(_file_record, thumb) do
    path = thumb["path"]
    full_path = Path.join(base_path(), path)

    case File.read(full_path) do
      {:ok, binary} -> {:ok, binary}
      {:error, reason} -> {:error, reason}
    end
  end

  # ── Thumbnail helpers ────────────────────────────────

  defp image?(filename), do: Path.extname(filename) in @image_exts

  # Parse "50x50", "480x720", or a single dimension "300" (keep aspect).
  defp parse_sizes(sizes) do
    parsed =
      sizes
      |> Enum.map(&String.trim/1)
      |> Enum.reject(&(&1 == ""))
      |> Enum.map(fn s ->
        case Regex.run(~r/^(\d+)(?:x(\d+))?$/, s) do
          [_, w, h] -> {s, "#{w}x#{h}"}
          [_, w] -> {s, "#{w}"}
          _ -> nil
        end
      end)
      |> Enum.reject(&is_nil/1)

    if parsed == [], do: {:error, :no_sizes}, else: {:ok, parsed}
  end

  defp find_magick do
    candidates = ["magick", "convert"]

    Enum.find_value(candidates, :error, fn cmd ->
      case System.find_executable(cmd) do
        nil -> nil
        path -> {:ok, path}
      end
    end)
  end

  defp make_thumb(magick, tmp_in, tmp_dir, size, geometry) do
    ext = ".webp"
    tmp_out = Path.join(tmp_dir, "lazypock-thumb-#{size}")
    rel_dir = Path.join([date_based_path(), "thumbs"])
    dir = Path.join(base_path(), rel_dir)
    File.mkdir_p!(dir)
    final = Path.join(dir, "#{thumb_basename(tmp_in)}-#{size}#{ext}")

    try do
      args = [tmp_in, "-auto-orient", "-resize", geometry, "-quality", "85", tmp_out]

      case System.cmd(magick, args, stderr_to_stdout: true) do
        {_out, 0} ->
          File.rename(tmp_out, final)
          dims = identify_size(magick, final)
          {w, h} = dims || {0, 0}

          %{
            "size" => size,
            "path" => Path.join(rel_dir, Path.basename(final)),
            "width" => w,
            "height" => h,
            "mime_type" => "image/webp"
          }

        {_out, _} ->
          File.rm(tmp_out)
          nil
      end
    rescue
      _ ->
        File.rm(tmp_out)
        nil
    end
  end

  defp identify_size(magick, path) do
    case System.cmd(magick, ["identify", "-format", "%w %h", path], stderr_to_stdout: true) do
      {out, 0} ->
        case String.split(String.trim(out), " ") do
          [w, h] -> {String.to_integer(w), String.to_integer(h)}
          _ -> nil
        end

      _ ->
        nil
    end
  rescue
    _ -> nil
  end

  defp thumb_basename(tmp_in) do
    tmp_in |> Path.basename() |> String.replace_prefix("lazypock-in", "thumb")
  end

  defp thumb_paths(file_record) do
    case file_record["thumbs"] do
      thumbs when is_map(thumbs) -> Enum.map(thumbs, fn {_k, v} -> v["path"] end)
      _ -> []
    end
  end

  defp temp_path(prefix) do
    Path.join(System.tmp_dir!(), "#{prefix}-#{Ecto.UUID.generate()}")
  end

  defp base_path do
    Application.get_env(:lazypock, :file_storage)[:path] ||
      Path.join(Application.app_dir(:lazypock, "priv"), "uploads")
  end

  defp date_based_path do
    now = DateTime.utc_now()
    "#{now.year}/#{pad(now.month)}/#{pad(now.day)}"
  end

  defp pad(n) when n < 10, do: "0#{n}"
  defp pad(n), do: to_string(n)

  defp mime_type(".jpg"), do: "image/jpeg"
  defp mime_type(".jpeg"), do: "image/jpeg"
  defp mime_type(".png"), do: "image/png"
  defp mime_type(".gif"), do: "image/gif"
  defp mime_type(".webp"), do: "image/webp"
  defp mime_type(".svg"), do: "image/svg+xml"
  defp mime_type(".pdf"), do: "application/pdf"
  defp mime_type(".mp4"), do: "video/mp4"
  defp mime_type(".mp3"), do: "audio/mpeg"
  defp mime_type(".json"), do: "application/json"
  defp mime_type(".csv"), do: "text/csv"
  defp mime_type(".txt"), do: "text/plain"
  defp mime_type(".zip"), do: "application/zip"
  defp mime_type(_), do: "application/octet-stream"

  defp clean_empty_dirs(dir) do
    case File.ls(dir) do
      {:ok, []} ->
        File.rmdir(dir)
        clean_empty_dirs(Path.dirname(dir))

      _ ->
        :ok
    end
  rescue
    _ -> :ok
  end
end
