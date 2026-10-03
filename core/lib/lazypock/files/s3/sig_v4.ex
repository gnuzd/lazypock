defmodule Lazypock.Files.S3.SigV4 do
  @moduledoc """
  Minimal AWS Signature Version 4 signer for S3-compatible endpoints.

  Implemented directly on `:crypto` so the project does not need an S3 client
  dependency (the single Burrito binary stays lean). Verified against the
  published AWS example in `SigV4Test`.

  The payload is signed with the *real* SHA-256 of the body, streamed from disk
  (`sha256_file/1`), rather than `UNSIGNED-PAYLOAD` — it is universally accepted
  and does not buffer the file in memory.
  """

  @algorithm "AWS4-HMAC-SHA256"

  # SHA-256 of an empty payload.
  @empty_sha256 "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"

  @doc "SHA-256 of a zero-length body, hex encoded."
  def empty_sha256, do: @empty_sha256

  @doc "Hex-encoded SHA-256 of `data`."
  def sha256_hex(data), do: :crypto.hash(:sha256, data) |> Base.encode16(case: :lower)

  @doc """
  Hex-encoded SHA-256 of a file, streamed in chunks so the file is never loaded
  into memory.
  """
  def sha256_file(path) do
    path
    |> File.stream!(64 * 1024, [])
    |> Enum.reduce(:crypto.hash_init(:sha256), fn chunk, ctx ->
      :crypto.hash_update(ctx, chunk)
    end)
    |> :crypto.hash_final()
    |> Base.encode16(case: :lower)
  end

  @doc """
  Signs a request and returns the headers to send (including `authorization`).

  ## Options

    * `:access_key_id` / `:secret_access_key` (required)
    * `:region` (default `"us-east-1"`)
    * `:service` (default `"s3"`)
    * `:datetime` — `DateTime` to sign for (default `DateTime.utc_now()`)
    * `:payload_hash` — hex SHA-256 of the body (default: empty-body hash)
    * `:sign_headers` — header names to sign (default
      `["host", "x-amz-content-sha256", "x-amz-date"]`)
  """
  @spec sign(String.t(), String.t(), map(), keyword()) :: map()
  def sign(method, url, headers, opts) do
    method = String.upcase(method)
    uri = URI.parse(url)
    datetime = Keyword.get(opts, :datetime, DateTime.utc_now())
    amz_datetime = format_datetime(datetime)
    date = String.slice(amz_datetime, 0, 8)
    payload_hash = Keyword.get(opts, :payload_hash, @empty_sha256)

    headers =
      headers
      |> normalize_headers()
      |> Map.put_new("host", host_header(uri))
      |> Map.put_new("x-amz-content-sha256", payload_hash)
      |> Map.put_new("x-amz-date", amz_datetime)

    sign_headers =
      (Keyword.get(opts, :sign_headers, ["host", "x-amz-content-sha256", "x-amz-date"]) ++
         Map.keys(headers))
      |> Enum.map(&String.downcase/1)
      |> Enum.uniq()
      |> Enum.filter(&Map.has_key?(headers, &1))
      |> Enum.sort()

    canonical_headers =
      Enum.map_join(sign_headers, "\n", fn name ->
        "#{name}:#{headers |> Map.fetch!(name) |> to_string() |> String.trim()}"
      end) <> "\n"

    signed_headers = Enum.join(sign_headers, ";")

    canonical_request =
      [
        method,
        canonical_uri(uri.path),
        canonical_query(uri.query),
        canonical_headers,
        signed_headers,
        payload_hash
      ]
      |> Enum.join("\n")

    scope = "#{date}/#{region(opts)}/#{service(opts)}/aws4_request"

    string_to_sign =
      [@algorithm, amz_datetime, scope, sha256_hex(canonical_request)] |> Enum.join("\n")

    signature =
      opts
      |> signing_key(date)
      |> hmac(string_to_sign)
      |> Base.encode16(case: :lower)

    authorization =
      "#{@algorithm} Credential=#{Keyword.fetch!(opts, :access_key_id)}/#{scope}, " <>
        "SignedHeaders=#{signed_headers}, Signature=#{signature}"

    Map.put(headers, "authorization", authorization)
  end

  @doc """
  Creates a presigned URL (query-string signing).

  ## Options

    * `:access_key_id` / `:secret_access_key` (required)
    * `:expires_in` — validity in seconds (default 900)
    * `:region` / `:service` / `:datetime` — as in `sign/4`
    * `:headers` — extra headers to sign (e.g. `content-type`, `content-length`)
    * `:sign_headers` — header names to sign (default `["host"]`)
    * `:content_sha256` — adds `X-Amz-Content-Sha256`; otherwise the payload is
      `UNSIGNED-PAYLOAD`, which is what presigned PUTs use
  """
  @spec presign(String.t(), String.t(), keyword()) :: String.t()
  def presign(method, url, opts) do
    method = String.upcase(method)
    uri = URI.parse(url)
    datetime = Keyword.get(opts, :datetime, DateTime.utc_now())
    amz_datetime = format_datetime(datetime)
    date = String.slice(amz_datetime, 0, 8)
    expires = Keyword.get(opts, :expires_in, 900)
    payload_hash = Keyword.get(opts, :payload_hash, "UNSIGNED-PAYLOAD")
    scope = "#{date}/#{region(opts)}/#{service(opts)}/aws4_request"

    headers =
      opts
      |> Keyword.get(:headers, %{})
      |> normalize_headers()
      |> Map.put_new("host", host_header(uri))

    sign_headers =
      opts
      |> Keyword.get(:sign_headers, ["host"])
      |> Enum.map(&String.downcase/1)
      |> Enum.sort()

    query =
      (uri.query || "")
      |> URI.query_decoder()
      |> Enum.into(%{})
      |> Map.merge(%{
        "X-Amz-Algorithm" => @algorithm,
        "X-Amz-Credential" => "#{Keyword.fetch!(opts, :access_key_id)}/#{scope}",
        "X-Amz-Date" => amz_datetime,
        "X-Amz-Expires" => Integer.to_string(expires),
        "X-Amz-SignedHeaders" => Enum.join(sign_headers, ";")
      })
      |> maybe_put("X-Amz-Content-Sha256", Keyword.get(opts, :content_sha256))

    canonical_query =
      query
      |> Enum.map(fn {k, v} -> {encode_param(k), encode_param(v)} end)
      |> Enum.sort()
      |> Enum.map_join("&", fn {k, v} -> "#{k}=#{v}" end)

    canonical_headers =
      Enum.map_join(sign_headers, "\n", fn name ->
        "#{name}:#{headers |> Map.fetch!(name) |> to_string() |> String.trim()}"
      end) <> "\n"

    canonical_request =
      [
        method,
        canonical_uri(uri.path),
        canonical_query,
        canonical_headers,
        Enum.join(sign_headers, ";"),
        payload_hash
      ]
      |> Enum.join("\n")

    string_to_sign =
      [@algorithm, amz_datetime, scope, sha256_hex(canonical_request)] |> Enum.join("\n")

    signature =
      opts
      |> signing_key(date)
      |> hmac(string_to_sign)
      |> Base.encode16(case: :lower)

    "#{uri.scheme}://#{host_header(uri)}#{canonical_uri(uri.path)}?#{canonical_query}" <>
      "&X-Amz-Signature=#{signature}"
  end

  defp maybe_put(map, _key, nil), do: map
  defp maybe_put(map, key, value), do: Map.put(map, key, value)

  @doc "UTC `YYYYMMDDTHHMMSSZ` timestamp used in the signing scope."
  def format_datetime(%DateTime{} = datetime) do
    datetime
    |> DateTime.to_naive()
    |> NaiveDateTime.to_erl()
    |> :calendar.datetime_to_gregorian_seconds()
    |> gregorian_to_amz()
  end

  defp gregorian_to_amz(seconds) do
    {{y, mo, d}, {h, mi, s}} =
      seconds |> :calendar.gregorian_seconds_to_datetime() |> then(& &1)

    :io_lib.format("~4..0B~2..0B~2..0BT~2..0B~2..0B~2..0BZ", [y, mo, d, h, mi, s])
    |> IO.iodata_to_binary()
  end

  defp signing_key(opts, date) do
    ("AWS4" <> Keyword.fetch!(opts, :secret_access_key))
    |> hmac(date)
    |> hmac(region(opts))
    |> hmac(service(opts))
    |> hmac("aws4_request")
  end

  defp hmac(key, data), do: :crypto.mac(:hmac, :sha256, key, data)

  defp region(opts), do: Keyword.get(opts, :region, "us-east-1")
  defp service(opts), do: Keyword.get(opts, :service, "s3")

  defp normalize_headers(headers) do
    Map.new(headers, fn {k, v} -> {k |> to_string() |> String.downcase(), to_string(v)} end)
  end

  defp host_header(%URI{host: host, port: port, scheme: scheme}) do
    cond do
      port == nil -> host
      scheme == "https" and port == 443 -> host
      scheme == "http" and port == 80 -> host
      true -> "#{host}:#{port}"
    end
  end

  # Each path segment is URI-encoded once (the incoming path may already be
  # percent-encoded, so decode first to avoid double-encoding), while `/`
  # separators are preserved.
  defp canonical_uri(nil), do: "/"
  defp canonical_uri(""), do: "/"

  defp canonical_uri(path) do
    path
    |> safe_decode()
    |> URI.encode(fn c -> c == ?/ or URI.char_unreserved?(c) end)
  end

  defp safe_decode(path) do
    URI.decode(path)
  rescue
    _ -> path
  end

  defp canonical_query(nil), do: ""
  defp canonical_query(""), do: ""

  defp canonical_query(query) do
    query
    |> URI.query_decoder()
    |> Enum.map(fn {k, v} -> {encode_param(k), encode_param(v)} end)
    |> Enum.sort()
    |> Enum.map_join("&", fn {k, v} -> "#{k}=#{v}" end)
  end

  defp encode_param(value) do
    URI.encode(value, &URI.char_unreserved?/1)
  end
end
