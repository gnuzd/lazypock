defmodule Lazypock.Files.Adapters.S3 do
  @moduledoc """
  S3-compatible storage adapter (AWS S3, Cloudflare R2, MinIO).

  Uses `Req` (already a dependency) plus the in-house `Lazypock.Files.S3.SigV4`
  signer, so no S3 client library is added to the release binary.

  Configuration comes from `Lazypock.Files.Storage` (Studio settings + env).
  Objects are stored under `<prefix><file_id>/` — `original.<ext>` plus preset
  variants — so one file's objects are a single prefix. Uploads stream from the
  Plug temp file, so the bytes are never buffered in the BEAM.

  Serving: `url/1` returns the public CDN URL when `public_base_url` is set,
  otherwise the app route (`/api/files/:id`), which proxies the object.
  """

  @behaviour Lazypock.Files.Adapter

  alias Lazypock.Files.Limiter
  alias Lazypock.Files.S3.SigV4
  alias Lazypock.Files.Storage

  @timeout 60_000

  # ── Store ────────────────────────────────────────────

  @impl true
  def store(source, filename, opts) do
    ext = filename |> Path.extname() |> String.downcase()
    id = opts[:id] || Ecto.UUID.generate()
    mime = opts[:mime_type] || Lazypock.Files.Adapters.Local.mime_type_for(ext)
    key = Storage.config()["prefix"] <> "#{id}/original#{ext}"

    with {:ok, path, cleanup} <- materialize(source) do
      try do
        case put_object(key, path, mime) do
          :ok ->
            size =
              case File.stat(path) do
                {:ok, %File.Stat{size: size}} -> size
                _ -> 0
              end

            {:ok, %{path: key, size: size, mime_type: mime}}

          {:error, reason} ->
            {:error, reason}
        end
      after
        cleanup.()
      end
    end
  end

  @impl true
  def put_at(storage_path, source, opts) do
    mime = opts[:mime_type] || "application/octet-stream"

    with {:ok, path, cleanup} <- materialize(source) do
      try do
        put_object(storage_path, path, mime)
      after
        cleanup.()
      end
    end
  end

  # ── Read / serve ─────────────────────────────────────

  @impl true
  def get(file_record) do
    case get_object(file_record["storage_path"]) do
      {:ok, body} -> {:ok, body}
      {:error, reason} -> {:error, reason}
    end
  end

  @impl true
  def thumb_get(_file_record, thumb) do
    case get_object(thumb["path"]) do
      {:ok, body} -> {:ok, body}
      {:error, reason} -> {:error, reason}
    end
  end

  @impl true
  def url(file_record) do
    case public_base_url() do
      nil -> "/api/files/#{file_record["id"]}"
      base -> base <> "/" <> encode_key(file_record["storage_path"])
    end
  end

  @impl true
  def local_path(_file_record), do: :error

  # Thumbnails in the legacy `thumbs` column are a local-disk concept; on S3 the
  # preset variants (see `scale/2`) are used instead.
  @impl true
  def thumbs(_source, _filename, _sizes), do: {:ok, []}

  # ── Variants ─────────────────────────────────────────

  @impl true
  def scale(file_record, %{"name" => name} = preset) do
    key = variant_key(file_record, "#{name}.webp")
    variant(file_record, key, preset, preset["quality"] || 80)
  end

  def scale(file_record, size) when is_binary(size) do
    safe = String.replace(size, ~r/[^A-Za-z0-9]/, "_")
    key = variant_key(file_record, "scale-#{safe}.webp")
    variant(file_record, key, size, 85)
  end

  defp variant(file_record, key, op, quality) do
    case get_object(key) do
      {:ok, binary} ->
        {:ok, binary, "image/webp"}

      {:error, :not_found} ->
        # Double-checked under the limiter: concurrent requests for the same
        # variant render once and the rest hit the object store.
        case Limiter.run(fn -> generate_variant(file_record, key, op, quality) end) do
          {:ok, result} -> result
          {:error, :overloaded} -> {:error, :overloaded}
        end

      {:error, reason} ->
        {:error, reason}
    end
  end

  defp generate_variant(file_record, key, op, quality) do
    case get_object(key) do
      {:ok, binary} ->
        {:ok, binary, "image/webp"}

      {:error, _} ->
        with {:ok, source, source_cleanup} <- download_temp(file_record["storage_path"]) do
          try do
            with {:ok, rendered, render_cleanup} <- render_temp(source, op, quality) do
              try do
                case put_object(key, rendered, "image/webp") do
                  :ok -> {:ok, File.read!(rendered), "image/webp"}
                  {:error, reason} -> {:error, reason}
                end
              after
                render_cleanup.()
              end
            end
          after
            source_cleanup.()
          end
        end
    end
  end

  defp render_temp(source, op, quality) do
    dest = temp_path("lazypock-render", ".webp")

    case Lazypock.Images.engine().resize(source, op, dest, quality: quality) do
      :ok -> {:ok, dest, fn -> File.rm(dest) end}
      {:error, reason} -> {:error, reason}
    end
  end

  # ── Delete ───────────────────────────────────────────

  @impl true
  def delete(file_record) do
    key = file_record["storage_path"]

    with :ok <- delete_object(key),
         :ok <- delete_prefix(prefix_for_key(key)) do
      :ok
    end
  end

  # ── Test connection ──────────────────────────────────

  @doc """
  Runs a real round-trip against the configured bucket:
  `PUT` a tiny object, `HEAD` it, `GET` it, then `DELETE` it.

  Returns a list of `%{step: atom, ok: boolean, error: term}` so the Studio can
  show exactly which step failed (credentials, endpoint, permissions, CORS).
  """
  def test_connection do
    key = "#{Storage.config()["prefix"]}.lazypock-connection-test"

    {:ok, path, cleanup} = materialize({:binary, "lazypock"})

    try do
      steps = [
        {:put, fn -> put_object(key, path, "text/plain") end},
        {:head, fn -> head_object(key) end},
        {:get, fn -> get_object(key) end},
        {:delete, fn -> delete_object(key) end}
      ]

      Enum.map(steps, fn {step, fun} ->
        case fun.() do
          :ok -> %{step: step, ok: true, error: nil}
          {:ok, _body} -> %{step: step, ok: true, error: nil}
          {:error, reason} -> %{step: step, ok: false, error: format_error(reason)}
        end
      end)
    after
      cleanup.()
    end
  end

  # ── HTTP plumbing ────────────────────────────────────

  # `path` is a local file already materialised by the caller; the body is
  # streamed from disk, so the bytes are never buffered in the BEAM.
  defp put_object(key, path, content_type) do
    payload_hash = SigV4.sha256_file(path)

    # The body is streamed from disk, so Req/Finch cannot infer its length and
    # would send it chunked — which Cloudflare R2 rejects with `411 Length
    # Required` (intermittently, depending on the connection). Set
    # `content-length` explicitly; SigV4 then signs it along with the rest of
    # the headers.
    size = File.stat!(path).size

    case request(:put, key,
           body: File.stream!(path, 1024 * 1024, []),
           headers: %{
             "content-type" => content_type,
             "content-length" => Integer.to_string(size)
           },
           payload_hash: payload_hash,
           sign_headers: ["host", "x-amz-content-sha256", "x-amz-date", "content-type"]
         ) do
      {:ok, %{status: status}} when status in 200..299 -> :ok
      {:ok, %{status: status}} -> {:error, {:http, status}}
      {:error, reason} -> {:error, reason}
    end
  end

  defp get_object(key) do
    case request(:get, key, []) do
      {:ok, %{status: status, body: body}} when status in 200..299 -> {:ok, body}
      {:ok, %{status: 404}} -> {:error, :not_found}
      {:ok, %{status: status}} -> {:error, {:http, status}}
      {:error, reason} -> {:error, reason}
    end
  end

  defp head_object(key) do
    case request(:head, key, []) do
      {:ok, %{status: status}} when status in 200..299 -> :ok
      {:ok, %{status: status}} -> {:error, {:http, status}}
      {:error, reason} -> {:error, reason}
    end
  end

  defp delete_object(key) do
    case request(:delete, key, []) do
      {:ok, %{status: status}} when status in 200..299 -> :ok
      {:ok, %{status: 404}} -> :ok
      {:ok, %{status: status}} -> {:error, {:http, status}}
      {:error, reason} -> {:error, reason}
    end
  end

  # Delete every object under a prefix (an aborted or partial variant set).
  defp delete_prefix(prefix) do
    case list_keys(prefix) do
      {:ok, keys} ->
        Enum.reduce_while(keys, :ok, fn key, :ok ->
          case delete_object(key) do
            :ok -> {:cont, :ok}
            {:error, _} = error -> {:halt, error}
          end
        end)

      {:error, reason} ->
        {:error, reason}
    end
  end

  defp list_keys(prefix) do
    query = URI.encode_query(%{"list-type" => "2", "prefix" => prefix, "max-keys" => "1000"})

    case request(:get, "", query: query) do
      {:ok, %{status: status, body: body}} when status in 200..299 ->
        {:ok, extract_keys(to_string(body))}

      {:ok, %{status: status}} ->
        {:error, {:http, status}}

      {:error, reason} ->
        {:error, reason}
    end
  end

  # ListObjectsV2 responses are simple XML; keys are URL-encoded, so decode them.
  defp extract_keys(xml) do
    Regex.scan(~r|<Key>(.*?)</Key>|s, xml)
    |> Enum.map(fn [_, key] -> key |> unescape_xml() |> URI.decode() end)
  end

  defp unescape_xml(value) do
    value
    |> String.replace("&lt;", "<")
    |> String.replace("&gt;", ">")
    |> String.replace("&quot;", "\"")
    |> String.replace("&#39;", "'")
    |> String.replace("&amp;", "&")
  end

  defp request(method, key, opts) do
    config = Storage.config()

    case build_url(config, key, opts[:query]) do
      {:error, reason} ->
        {:error, reason}

      {:ok, url} ->
        headers = opts[:headers] || %{}

        signed =
          SigV4.sign(
            method_name(method),
            url,
            headers,
            sigv4_opts(config, opts[:payload_hash], opts[:sign_headers])
          )

        request_opts =
          [
            method: method,
            url: url,
            headers: signed,
            decode_body: false,
            receive_timeout: @timeout,
            retry: :transient,
            max_retries: 2
          ] ++
            Keyword.take(opts, [:body, :into])

        request_opts = Keyword.merge(request_opts, configured_req_options())

        case Req.request(Req.new(request_opts)) do
          {:ok, response} -> {:ok, response}
          {:error, exception} -> {:error, exception}
        end
    end
  end

  # Test seam / proxy support: `config :lazypock, Lazypock.Files.Adapters.S3,
  # req_options: [...]` is merged into every request (e.g. a `Req.Test` plug).
  defp configured_req_options do
    Application.get_env(:lazypock, __MODULE__, []) |> Keyword.get(:req_options, [])
  end

  defp sigv4_opts(config, payload_hash, sign_headers) do
    [
      access_key_id: config["access_key_id"],
      secret_access_key: config["secret_access_key"],
      region: config["region"] || "us-east-1",
      service: "s3"
    ]
    |> maybe_put(:payload_hash, payload_hash)
    |> maybe_put(:sign_headers, sign_headers)
  end

  defp maybe_put(opts, _key, nil), do: opts
  defp maybe_put(opts, key, value), do: Keyword.put(opts, key, value)

  defp method_name(:get), do: "GET"
  defp method_name(:put), do: "PUT"
  defp method_name(:head), do: "HEAD"
  defp method_name(:delete), do: "DELETE"

  defp build_url(config, key, query) do
    cond do
      is_nil(config["endpoint"]) or config["endpoint"] == "" ->
        {:error, :not_configured}

      is_nil(config["bucket"]) or config["bucket"] == "" ->
        {:error, :not_configured}

      true ->
        uri = URI.parse(config["endpoint"])
        scheme = uri.scheme || "https"
        host = uri.host

        base =
          if config["force_path_style"] == false do
            "#{scheme}://#{config["bucket"]}.#{host}"
          else
            "#{scheme}://#{host}#{uri.port && ":#{uri.port}"}"
          end

        path =
          if config["force_path_style"] == false do
            "/" <> key
          else
            "/" <> config["bucket"] <> "/" <> key
          end

        url = base <> URI.encode(path, fn c -> c == ?/ or URI.char_unreserved?(c) end)

        {:ok, if(query, do: url <> "?" <> query, else: url)}
    end
  end

  @doc false
  def encode_key(key), do: URI.encode(key, fn c -> c == ?/ or URI.char_unreserved?(c) end)

  @doc "CDN URL for a preset variant, or the app route when no public base URL."
  def variant_url(file_record, name) do
    case public_base_url() do
      nil -> "/api/files/#{file_record["id"]}/scale/#{name}"
      base -> base <> "/" <> encode_key(variant_key(file_record, "#{name}.webp"))
    end
  end

  # ── Direct upload support (P4) ───────────────────────

  @doc "Object key for a direct upload's original."
  def object_key(id, filename) do
    ext = filename |> Path.extname() |> String.downcase()
    Storage.config()["prefix"] <> "#{id}/original#{ext}"
  end

  @doc """
  Presigns a `PUT` for a direct upload.

  `content-type` and `content-length` are signed to fixed values (R2 has no
  presigned-POST content-length-range), so the completed object is re-verified
  by `complete/2`.
  """
  def presign_put(key, content_type, content_length) do
    config = Storage.config()

    with {:ok, url} <- build_url(config, key, nil) do
      headers = %{"content-type" => content_type, "content-length" => to_string(content_length)}

      presigned =
        SigV4.presign(
          "PUT",
          url,
          base_presign_opts(config) ++
            [
              expires_in: config["presign_ttl"] || 900,
              headers: headers,
              sign_headers: ["host", "content-type", "content-length"]
            ]
        )

      {:ok, %{method: "PUT", url: presigned, headers: headers}}
    end
  end

  @doc "Presigns a `GET` (used for protected files)."
  def presign_get(key, expires_in \\ nil) do
    config = Storage.config()

    with {:ok, url} <- build_url(config, key, nil) do
      {:ok,
       SigV4.presign(
         "GET",
         url,
         base_presign_opts(config) ++
           [expires_in: expires_in || config["presign_ttl"] || 900, sign_headers: ["host"]]
       )}
    end
  end

  @doc "Object size from a HEAD request (`{:ok, nil}` when the server omits it)."
  def head_size(key) do
    case request(:head, key, []) do
      {:ok, %{status: status, headers: headers}} when status in 200..299 ->
        {:ok, content_length(headers)}

      {:ok, %{status: 404}} ->
        {:error, :not_found}

      {:ok, %{status: status}} ->
        {:error, {:http, status}}

      {:error, reason} ->
        {:error, reason}
    end
  end

  @doc "Streams an object to a local file."
  def download_to(key, dest) do
    case request(:get, key, into: File.stream!(dest, 1024 * 1024, [:write])) do
      {:ok, %{status: status}} when status in 200..299 -> :ok
      {:ok, %{status: 404}} -> {:error, :not_found}
      {:ok, %{status: status}} -> {:error, {:http, status}}
      {:error, reason} -> {:error, reason}
    end
  end

  defp base_presign_opts(config) do
    [
      access_key_id: config["access_key_id"],
      secret_access_key: config["secret_access_key"],
      region: config["region"] || "us-east-1",
      service: "s3"
    ]
  end

  defp content_length(headers) when is_map(headers) do
    headers
    |> Map.get("content-length")
    |> List.wrap()
    |> List.first()
    |> parse_int()
  end

  defp content_length(_), do: nil

  defp parse_int(nil), do: nil

  defp parse_int(value) when is_binary(value) do
    case Integer.parse(value) do
      {n, _} -> n
      :error -> nil
    end
  end

  defp public_base_url do
    case Storage.config()["public_base_url"] do
      nil -> nil
      "" -> nil
      base -> String.trim_trailing(base, "/")
    end
  end

  defp variant_key(file_record, name) do
    original = file_record["storage_path"]
    dir = Path.dirname(original)
    if dir in [".", ""], do: name, else: dir <> "/" <> name
  end

  defp prefix_for_key(key) do
    dir = Path.dirname(key)
    if dir in [".", ""], do: "", else: dir <> "/"
  end

  defp materialize({:file, path}), do: {:ok, path, fn -> :ok end}

  defp materialize({:binary, binary}) when is_binary(binary) do
    tmp = temp_path("lazypock-s3", ".bin")
    File.write!(tmp, binary)
    {:ok, tmp, fn -> File.rm(tmp) end}
  end

  defp materialize(binary) when is_binary(binary) do
    tmp = temp_path("lazypock-s3", ".bin")
    File.write!(tmp, binary)
    {:ok, tmp, fn -> File.rm(tmp) end}
  end

  defp download_temp(key) do
    dest = temp_path("lazypock-s3-download", ".bin")

    case request(:get, key, into: File.stream!(dest, 1024 * 1024, [:write])) do
      {:ok, %{status: status}} when status in 200..299 -> {:ok, dest, fn -> File.rm(dest) end}
      {:ok, %{status: 404}} -> {:error, :not_found}
      {:ok, %{status: status}} -> {:error, {:http, status}}
      {:error, reason} -> {:error, reason}
    end
  end

  defp temp_path(prefix, suffix) do
    Path.join(System.tmp_dir!(), "#{prefix}-#{Ecto.UUID.generate()}#{suffix}")
  end

  defp format_error({:http, status}) when is_integer(status), do: "HTTP #{status}"
  defp format_error(:not_found), do: "object not found (404)"
  defp format_error(:not_configured), do: "storage is not configured"
  defp format_error(other), do: inspect(other)
end
