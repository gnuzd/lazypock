defmodule Lazypock.Files.Adapters.Local do
  @moduledoc """
  Local filesystem adapter.

  Stores files under `base_path/0` (`priv/uploads/` by default, or
  `LAZYPOCK_STORAGE_PATH`) organized by date:
    <base>/YYYY/MM/DD/{uuid}.{ext}

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

    # Remove preset variants for this file id
    file_record |> variant_dir() |> File.rm_rf()

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
            if Lazypock.Images.available?() do
              generate_thumbs(source, geometry)
            else
              warn_missing_magick()
              {:ok, []}
            end
        end
    end
  end

  # Thumbnail generation is best-effort but must not oversubscribe the image
  # slots: a reload under load skips thumbnails rather than queueing forever.
  defp generate_thumbs(source, geometry) do
    case Lazypock.Files.Limiter.run(fn -> do_generate_thumbs(source, geometry) end) do
      {:ok, result} -> result
      {:error, :overloaded} -> {:ok, []}
    end
  end

  defp do_generate_thumbs(source, geometry) do
    with {:ok, input, cleanup} <- materialize(source) do
      try do
        results =
          geometry
          |> Enum.map(fn {size, geom} -> make_thumb(input, size, geom) end)
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

  @doc false
  def variant_url(file_record, name), do: "/api/files/#{file_record["id"]}/scale/#{name}"

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
  def scale(file_record, preset) when is_map(preset) do
    with true <- image?(file_record["filename"] || "") || {:error, :not_an_image},
         {:ok, source_path} <- local_path(file_record),
         :ok <- ensure_file(source_path) do
      dest = variant_path(file_record, preset["name"])
      cached_or_render(source_path, dest, preset, preset["quality"] || 80)
    else
      {:error, reason} -> {:error, reason}
    end
  end

  @impl true
  def scale(file_record, size) when is_binary(size) do
    with {:ok, geometry} <- parse_scale_size(size),
         true <- image?(file_record["filename"] || "") || {:error, :not_an_image},
         {:ok, source_path} <- local_path(file_record),
         :ok <- ensure_file(source_path) do
      cached_or_render(source_path, scale_cache_path(file_record, size), geometry, 85)
    else
      {:error, reason} -> {:error, reason}
    end
  end

  defp ensure_file(path) do
    if File.regular?(path), do: :ok, else: {:error, :enoent}
  end

  defp cached_or_render(source, dest, op, quality) do
    case File.read(dest) do
      {:ok, binary} ->
        {:ok, binary, "image/webp"}

      {:error, _} ->
        render_under_limiter(source, dest, op, quality)
    end
  end

  # Double-checked: after waiting for a slot, re-check the cache so concurrent
  # requests for the same variant result in a single generation.
  defp render_under_limiter(source, dest, op, quality) do
    case Lazypock.Files.Limiter.run(fn ->
           case File.read(dest) do
             {:ok, binary} ->
               {:ok, binary, "image/webp"}

             {:error, _} ->
               case render(source, op, dest, quality) do
                 :ok -> {:ok, File.read!(dest), "image/webp"}
                 {:error, reason} -> {:error, reason}
               end
           end
         end) do
      {:ok, result} -> result
      {:error, :overloaded} -> {:error, :overloaded}
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

  # Named, preset variants live under one directory per file id.
  defp variant_path(file_record, name) do
    Path.join([variant_dir(file_record), "#{name}.webp"])
  end

  defp variant_dir(file_record) do
    id = file_record["id"] |> to_string() |> String.replace("-", "")
    Path.join([base_path(), "_variants", id])
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

  defp make_thumb(input, size, geometry) do
    rel_dir = Path.join([date_based_path(), "thumbs"])
    dir = Path.join(base_path(), rel_dir)
    File.mkdir_p!(dir)
    name = "thumb-#{Ecto.UUID.generate()}-#{size}.webp"
    final = Path.join(dir, name)

    case render(input, geometry, final, 85) do
      :ok ->
        {w, h} =
          case Lazypock.Images.engine().dimensions(final) do
            {:ok, w, h} -> {w, h}
            _ -> {0, 0}
          end

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
  rescue
    _ -> nil
  end

  # Render to a temp file in the destination directory, then rename, so no
  # concurrent reader ever sees a half-written variant.
  defp render(source, op, dest, quality) do
    File.mkdir_p!(Path.dirname(dest))
    tmp = dest <> ".tmp-#{Ecto.UUID.generate()}"

    try do
      case Lazypock.Images.engine().resize(source, op, tmp, quality: quality) do
        :ok ->
          case File.rename(tmp, dest) do
            :ok -> :ok
            {:error, reason} -> {:error, reason}
          end

        {:error, reason} ->
          {:error, reason}
      end
    after
      File.rm(tmp)
    end
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

  @doc """
  Root directory for local storage.

  Resolved in order:

    1. `config :lazypock, :file_storage, path: "..."` (tests, embedded use)
    2. `LAZYPOCK_STORAGE_PATH` env var (containerized deployments)
    3. `<app_dir>/priv/uploads` (default)

  A release ships under a version-stamped directory
  (`.../lib/lazypock-<vsn>/priv/uploads`), so the default is wiped whenever the
  container is recreated or lazypock/ERTS is upgraded — the database rows
  survive (e.g. on Neon) while the bytes disappear. Point
  `LAZYPOCK_STORAGE_PATH` at a mounted volume (or use S3/R2) to keep uploads.
  """
  @spec base_path() :: String.t()
  def base_path do
    Application.get_env(:lazypock, :file_storage)[:path] ||
      env_storage_path() ||
      Path.join(Application.app_dir(:lazypock, "priv"), "uploads")
  end

  defp env_storage_path do
    case System.get_env("LAZYPOCK_STORAGE_PATH") do
      nil -> nil
      "" -> nil
      path -> path
    end
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

  @doc """
  MIME type for an extension (with or without the dot, any case). Exposed so
  other adapters resolve stored MIME types consistently.
  """
  def mime_type_for(extension) do
    ext = extension |> to_string() |> String.downcase()
    ext = if String.starts_with?(ext, "."), do: ext, else: "." <> ext
    mime_type(ext)
  end

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
