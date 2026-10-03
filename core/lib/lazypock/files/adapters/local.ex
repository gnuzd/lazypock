defmodule Lazypock.Files.Adapters.Local do
  @moduledoc """
  Local filesystem adapter.

  Stores files in `priv/uploads/` organized by date:
    priv/uploads/YYYY/MM/DD/{uuid}.{ext}

  Image resizing (thumbnails and on-demand scaling) uses ImageMagick
  (`magick` or `convert`) through `Lazypock.Files.Limiter`, so at most
  `LAZYPOCK_IMAGE_CONCURRENCY` resizes run at once per instance. If ImageMagick
  is not installed, uploads still work but no thumbnails/scaling are generated
  (a warning is logged once). Set the env var `LAZYPOCK_THUMBNAILS=0` to
  disable image resizing entirely.

  All resized output is encoded as WebP explicitly (`webp:` prefix), regardless
  of the source format — writing to an extensionless temp file lets ImageMagick
  inherit the *input* format (a JPEG payload served as `image/webp`).
  """

  require Logger

  @behaviour Lazypock.Files.Adapter

  @image_exts ~w(.jpg .jpeg .png .gif .webp .avif)

  # Largest single dimension an on-demand `/scale` (or a configured thumbnail)
  # may request. Without this, `GET /scale/:size` accepts any size up to 9999
  # and lets anyone force expensive resizes and fill the disk.
  @max_scale_dimension 2000

  @doc """
  Stores a file on the local filesystem.

  Accepts inline bytes or `{:file, path}` — the latter copies from disk so the
  caller never holds the whole upload in memory.
  """
  @impl true
  def store(source, filename, _opts) do
    ext = filename |> Path.extname() |> String.downcase()
    uuid = Ecto.UUID.generate()
    date_path = date_based_path()
    storage_path = Path.join([base_path(), date_path, "#{uuid}#{ext}"])
    File.mkdir_p!(Path.dirname(storage_path))

    case write_source(source, storage_path) do
      {:ok, size} ->
        {:ok,
         %{
           path: Path.join(date_path, "#{uuid}#{ext}"),
           size: size,
           mime_type: mime_type(ext)
         }}

      {:error, reason} ->
        {:error, reason}
    end
  end

  defp write_source({:file, src}, dest) do
    with_written_size(dest, fn -> File.cp(src, dest) end)
  end

  defp write_source(binary, dest) when is_binary(binary) do
    with_written_size(dest, fn -> File.write(dest, binary) end)
  end

  defp with_written_size(dest, fun) do
    case fun.() do
      :ok ->
        case File.stat(dest) do
          {:ok, %File.Stat{size: size}} -> {:ok, size}
          {:error, reason} -> {:error, reason}
        end

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

  # Backup support: expose the on-disk path so an export can `File.cp/2` a large
  # upload instead of reading it into memory.
  @impl true
  def local_path(file_record) do
    {:ok, Path.join(base_path(), file_record["storage_path"])}
  end

  # Backup support: write at an EXPLICIT storage path. `store/3` generates its own
  # path, which would orphan every `_files.storage_path` reference on restore.
  @impl true
  def put_at(storage_path, source, _opts) do
    full_path = Path.join(base_path(), storage_path)
    File.mkdir_p!(Path.dirname(full_path))

    case source do
      {:file, src} -> File.cp(src, full_path)
      binary when is_binary(binary) -> File.write(full_path, binary)
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

    # Remove cached on-demand variants for this file id
    file_record
    |> scale_cache_glob()
    |> Path.wildcard()
    |> Enum.each(&File.rm/1)

    # Try to clean up empty parent dirs (ignore errors)
    clean_empty_dirs(Path.dirname(full_path))
    :ok
  end

  @impl true
  def thumbs(source, filename, sizes) do
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
                generate_with_magick(magick, source, geometry)

              :error ->
                warn_missing_magick()
                {:ok, []}
            end
        end
    end
  end

  # Thumbnail generation is best-effort but must not oversubscribe the image
  # slots: a reload under load skips thumbnails rather than queueing forever.
  defp generate_with_magick(magick, source, geometry) do
    case Lazypock.Files.Limiter.run(fn -> do_generate_thumbs(magick, source, geometry) end) do
      {:ok, result} -> result
      {:error, :overloaded} -> {:ok, []}
    end
  end

  defp do_generate_thumbs(magick, source, geometry) do
    with {:ok, input, cleanup} <- materialize(source) do
      try do
        results =
          geometry
          |> Enum.map(fn {size, geom} -> make_thumb(magick, input, size, geom) end)
          |> Enum.reject(&is_nil/1)

        {:ok, results}
      after
        cleanup.()
      end
    end
  end

  # Hand ImageMagick a filesystem path. `{:file, path}` is used as-is (no copy
  # into the BEAM); inline bytes are written to a uniquely named temp file.
  defp materialize({:file, path}), do: {:ok, path, fn -> :ok end}

  defp materialize(binary) when is_binary(binary) do
    tmp = temp_path("lazypock-thumb-in", ".bin")
    File.write!(tmp, binary)
    {:ok, tmp, fn -> File.rm(tmp) end}
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

  @impl true
  def thumb_get(_file_record, thumb) do
    path = thumb["path"]
    full_path = Path.join(base_path(), path)

    case File.read(full_path) do
      {:ok, binary} -> {:ok, binary}
      {:error, reason} -> {:error, reason}
    end
  end

  @impl true
  def scale(file_record, size) do
    with {:ok, geometry} <- parse_scale_size(size),
         true <- image?(file_record["filename"] || "") || {:error, :not_an_image},
         {:ok, magick} <- find_magick(),
         {:ok, source_path} <- local_path(file_record),
         :ok <- ensure_file(source_path) do
      cached_path = scale_cache_path(file_record, size)

      case File.read(cached_path) do
        {:ok, binary} ->
          {:ok, binary, "image/webp"}

        {:error, _} ->
          generate_scale(magick, source_path, geometry, cached_path)
      end
    else
      {:error, reason} -> {:error, reason}
      false -> {:error, :not_an_image}
      :error -> {:error, :magick_not_found}
    end
  end

  defp ensure_file(path) do
    if File.regular?(path), do: :ok, else: {:error, :enoent}
  end

  defp generate_scale(magick, source_path, geometry, cached_path) do
    case Lazypock.Files.Limiter.run(fn ->
           do_scale(magick, source_path, geometry, cached_path)
         end) do
      {:ok, result} -> result
      {:error, :overloaded} -> {:error, :overloaded}
    end
  end

  defp do_scale(magick, source_path, geometry, cached_path) do
    File.mkdir_p!(Path.dirname(cached_path))
    # Same directory as the target so the rename is atomic and same-filesystem.
    tmp_out = cached_path <> ".tmp-#{Ecto.UUID.generate()}"

    try do
      case run_magick(magick, source_path, geometry, tmp_out) do
        :ok ->
          case File.rename(tmp_out, cached_path) do
            :ok -> {:ok, File.read!(cached_path), "image/webp"}
            {:error, reason} -> {:error, reason}
          end

        {:error, reason} ->
          {:error, reason}
      end
    rescue
      _ -> {:error, :resize_failed}
    after
      File.rm(tmp_out)
    end
  end

  # Stable, unbounded-lifetime cache path. Keying the cache by the current date
  # (the previous behaviour) re-encoded every variant daily and orphaned the
  # previous day's files.
  defp scale_cache_path(file_record, size) do
    id = file_record["id"] |> to_string() |> String.replace("-", "")
    safe_size = String.replace(size, ~r/[^A-Za-z0-9]/, "_")
    Path.join([base_path(), "_cache", "scale", "#{id}-#{safe_size}.webp"])
  end

  defp scale_cache_glob(file_record) do
    id = file_record["id"] |> to_string() |> String.replace("-", "")
    Path.join([base_path(), "_cache", "scale", "#{id}-*"])
  end

  # ImageMagick geometry: 300 | 300x | x300 | 300x200 | 300x200! (exact).
  # Dimensions are capped so a public `/scale/:size` cannot be used to force
  # arbitrarily expensive resizes.
  defp parse_scale_size(size) when is_binary(size) do
    s = String.trim(size)

    with :ok <- validate_geometry_shape(s),
         :ok <- validate_geometry_range(s) do
      {:ok, s}
    end
  end

  defp parse_scale_size(_), do: {:error, :invalid_size}

  defp validate_geometry_shape(s) do
    if Regex.match?(~r/^(?:\d{1,5}x\d{1,5}!?|\d{1,5}x?|x\d{1,5})$/, s),
      do: :ok,
      else: {:error, :invalid_size}
  end

  defp validate_geometry_range(s) do
    dims = ~r/\d+/ |> Regex.scan(s) |> List.flatten() |> Enum.map(&String.to_integer/1)

    if dims != [] and Enum.all?(dims, &(&1 >= 1 and &1 <= @max_scale_dimension)),
      do: :ok,
      else: {:error, :invalid_size}
  end

  # ── Thumbnail helpers ────────────────────────────────

  defp image?(filename) do
    filename |> Path.extname() |> String.downcase() |> then(&(&1 in @image_exts))
  end

  # Parse "50x50", "480x720", or a single dimension "300" (keep aspect).
  # Sizes above the dimension cap are dropped rather than handed to ImageMagick.
  defp parse_sizes(sizes) do
    parsed =
      sizes
      |> Enum.map(&String.trim/1)
      |> Enum.reject(&(&1 == ""))
      |> Enum.map(fn s ->
        case Regex.run(~r/^(\d+)(?:x(\d+))?$/, s) do
          [_, w, h] -> {s, "#{w}x#{h}", [w, h]}
          [_, w] -> {s, "#{w}", [w]}
          _ -> nil
        end
      end)
      |> Enum.reject(&is_nil/1)
      |> Enum.filter(fn {_s, _geom, dims} ->
        Enum.all?(dims, fn d -> String.to_integer(d) <= @max_scale_dimension end)
      end)
      |> Enum.map(fn {s, geom, _dims} -> {s, geom} end)

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

  defp make_thumb(magick, input, size, geometry) do
    rel_dir = Path.join([date_based_path(), "thumbs"])
    dir = Path.join(base_path(), rel_dir)
    File.mkdir_p!(dir)
    name = "thumb-#{Ecto.UUID.generate()}-#{size}.webp"
    final = Path.join(dir, name)
    tmp_out = Path.join(dir, ".tmp-#{Ecto.UUID.generate()}.webp")

    try do
      case run_magick(magick, input, geometry, tmp_out) do
        :ok ->
          case File.rename(tmp_out, final) do
            :ok ->
              {w, h} = identify_size(magick, final) || {0, 0}

              %{
                "size" => size,
                "path" => Path.join(rel_dir, name),
                "width" => w,
                "height" => h,
                "mime_type" => "image/webp"
              }

            {:error, _} ->
              nil
          end

        {:error, _} ->
          nil
      end
    rescue
      _ ->
        nil
    after
      File.rm(tmp_out)
    end
  end

  # All resizing goes through one place so the resource limits are applied
  # consistently, metadata is stripped, and the output format is fixed to WebP.
  defp run_magick(magick, input, geometry, output) do
    args = [
      "-limit",
      "memory",
      image_memory_limit(),
      "-limit",
      "map",
      image_memory_limit(),
      input,
      "-auto-orient",
      "-strip",
      "-resize",
      geometry,
      "-quality",
      "85",
      "webp:#{output}"
    ]

    case System.cmd(magick, args, stderr_to_stdout: true) do
      {_out, 0} ->
        :ok

      {out, code} ->
        Logger.debug("ImageMagick resize failed (#{code}): #{String.trim(out)}")
        {:error, :resize_failed}
    end
  rescue
    _ -> {:error, :resize_failed}
  end

  defp image_memory_limit do
    System.get_env("LAZYPOCK_MAGICK_MEMORY_LIMIT") || "256MiB"
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

  defp thumb_paths(file_record) do
    case file_record["thumbs"] do
      thumbs when is_map(thumbs) -> Enum.map(thumbs, fn {_k, v} -> v["path"] end)
      _ -> []
    end
  end

  defp temp_path(prefix, suffix) do
    Path.join(System.tmp_dir!(), "#{prefix}-#{Ecto.UUID.generate()}#{suffix}")
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
  defp mime_type(".avif"), do: "image/avif"
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
