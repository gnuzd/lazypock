defmodule Lazypock.CORS do
  @moduledoc """
  Dynamic CORS handling for LazyPock — PocketBase-style "Allowed Origins".

  Origins are resolved per-request from, in priority order:

    1. `LAZYPOCK_CORS_ORIGINS` env var (comma-separated) — server-level override
    2. `cors_origins` setting from the `_settings` DB table (editable in the
       Studio admin UI, no restart needed)
    3. The app's own origin (`http://localhost:PORT`) — always allowed, since
       the lazypock-ts SDK derives its socket/API URL from `baseUrl`

  The list is cached in-memory for a few seconds (`@ttl_ms`) so DB lookups
  don't happen on every request, but UI changes take effect almost
  immediately (no server restart).

  Used for both:
    * HTTP CORS headers (`plug Lazypock.CORS` in the endpoint)
    * Phoenix websocket `check_origin` (via the MFA
      `{Lazypock.CORS, :origin_allowed?, [...]}`)
  """

  alias Plug.Conn

  # How long to cache the merged origins list (ms). Settings changes appear
  # within this window without a restart.
  @ttl_ms 2_000

  # ── Plug callbacks ─────────────────────────────────────

  @behaviour Plug

  @impl true
  def init(opts), do: opts

  @impl true
  def call(conn, _opts) do
    origins = allowed_origins()
    raw_origin = request_origin(conn)
    origin = raw_origin && normalize_origin(raw_origin)

    cond do
      origins == [] or origin == nil ->
        conn

      Enum.member?(origins, origin) ->
        conn
        |> Conn.put_resp_header("access-control-allow-origin", raw_origin)
        |> maybe_credentials()
        |> Conn.put_resp_header("access-control-expose-headers", "Content-Disposition")
        |> maybe_preflight()

      "*" in origins ->
        conn
        |> Conn.put_resp_header("access-control-allow-origin", "*")
        |> maybe_preflight()

      true ->
        conn
    end
  end

  # CORS preflight requests (OPTIONS + Access-Control-Request-Method) must be
  # answered with 204 + allow headers, otherwise browsers reject the real
  # request before it is even sent.
  defp maybe_preflight(conn) do
    requested_method =
      conn
      |> Conn.get_req_header("access-control-request-method")
      |> List.first()

    if conn.method == "OPTIONS" and requested_method != nil do
      # Echo the browser's requested headers back (standard CORS practice)
      # instead of a hardcoded list, so custom headers like `X-Connection-Id`
      # (realtime origin-exclusion) are always allowed without maintaining
      # the list by hand.
      requested_headers =
        conn
        |> Conn.get_req_header("access-control-request-headers")
        |> List.first()
        |> case do
          nil -> "Content-Type, Authorization"
          value -> value
        end

      conn
      |> Conn.put_resp_header(
        "access-control-allow-methods",
        "GET, POST, PUT, PATCH, DELETE, OPTIONS"
      )
      |> Conn.put_resp_header("access-control-allow-headers", requested_headers)
      |> Conn.put_resp_header("access-control-max-age", "86400")
      |> Conn.send_resp(204, "")
      |> Conn.halt()
    else
      conn
    end
  end

  # ── Websocket check_origin MFA ─────────────────────────

  @doc """
  MFA callback for Phoenix `check_origin: {Lazypock.CORS, :origin_allowed?, [...]}`.
  Receives the parsed `%URI{}` and returns whether the origin is allowed.
  """
  def origin_allowed?(%URI{} = uri, _any) do
    origins = allowed_origins()

    if "*" in origins do
      true
    else
      origin = origin_string(uri)
      origin != nil and origin in origins
    end
  end

  def origin_allowed?(_other, _any), do: false

  # Build a normalized "scheme://host:port" string. Elixir's URI.parse leaves
  # port as nil when the URL has no explicit port; normalize to the scheme
  # default (80 for http, 443 for https) so "http://tauri.local" matches the
  # stored "http://tauri.local".
  defp origin_string(%URI{scheme: scheme, host: host} = uri)
       when is_binary(scheme) and is_binary(host) do
    port =
      case uri.port do
        nil when scheme == "https" -> 443
        nil -> 80
        p -> p
      end

    "#{scheme}://#{host}:#{port}"
  end

  defp origin_string(_), do: nil

  # ── Origins resolution ─────────────────────────────────

  @doc "The merged, deduplicated list of allowed origins (env + settings + own)."
  def allowed_origins do
    ensure_cache!()

    now = System.monotonic_time(:millisecond)

    case :ets.lookup(:lazypock_cors_cache, :origins) do
      [{:origins, origins, ts}] when now - ts < @ttl_ms -> origins
      _ -> refresh_origins()
    end
  end

  @doc "Force-refresh the cached origins (e.g. after a settings update)."
  def refresh_origins do
    ensure_cache!()

    origins =
      []
      |> Kernel.++(env_origins())
      |> Kernel.++(settings_origins())
      |> Kernel.++(own_origin())
      |> Enum.map(&normalize_origin/1)
      |> Enum.reject(&is_nil/1)
      |> Enum.uniq()

    :ets.insert(:lazypock_cors_cache, {:origins, origins, System.monotonic_time(:millisecond)})
    origins
  end

  # Normalize a stored origin string to "scheme://host:port" so it matches
  # what origin_string/1 produces for websocket URIs (default ports filled in).
  # The "*" wildcard passes through unchanged.
  defp normalize_origin("*"), do: "*"

  defp normalize_origin(origin) when is_binary(origin) do
    case URI.parse(String.trim(origin)) do
      %URI{scheme: scheme, host: host} when is_binary(scheme) and is_binary(host) ->
        origin_string(URI.parse(String.trim(origin)))

      _ ->
        nil
    end
  end

  defp normalize_origin(_), do: nil

  defp ensure_cache! do
    case :ets.whereis(:lazypock_cors_cache) do
      :undefined ->
        try do
          :ets.new(:lazypock_cors_cache, [:named_table, :public, :set, read_concurrency: true])
        rescue
          ArgumentError -> :ok
        end

      _ ->
        :ok
    end
  end

  # Env override (comma-separated). Quotes stripped for docker-compose.
  defp env_origins do
    case System.get_env("LAZYPOCK_CORS_ORIGINS") do
      nil ->
        []

      value ->
        value
        |> String.trim("\"")
        |> String.split(",")
        |> Enum.map(&String.trim/1)
        |> Enum.reject(&(&1 == ""))
    end
  end

  # Origins stored in the _settings table (Studio UI).
  defp settings_origins do
    case Lazypock.Settings.get("cors_origins") do
      origins when is_list(origins) -> origins |> Enum.map(&to_string/1)
      origins when is_binary(origins) -> origins |> String.split(",") |> Enum.map(&String.trim/1)
      _ -> []
    end
  rescue
    _ -> []
  end

  # The app's own HTTP origin — the lazypock-ts SDK sends this as the
  # websocket Origin, so it must always be allowed.
  defp own_origin do
    port = System.get_env("PORT", "4000")
    ["http://localhost:#{port}"]
  end

  defp request_origin(conn) do
    conn
    |> Conn.get_req_header("origin")
    |> List.first()
    |> case do
      nil -> nil
      origin -> String.trim(origin)
    end
  end

  defp maybe_credentials(conn) do
    if System.get_env("LAZYPOCK_CORS_CREDENTIALS", "true") == "true" do
      Conn.put_resp_header(conn, "access-control-allow-credentials", "true")
    else
      conn
    end
  end
end
