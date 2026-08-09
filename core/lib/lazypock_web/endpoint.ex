defmodule LazypockWeb.Endpoint do
  use Phoenix.Endpoint, otp_app: :lazypock

  @session_options [
    store: :cookie,
    key: "_lazypock_key",
    signing_salt: "6IsN8XOz",
    same_site: "Lax"
  ]

  # Realtime socket for collection events
  socket "/socket", LazypockWeb.CollectionSocket,
    websocket: true,
    longpoll: false

  # Serve SvelteKit SPA built assets (under /_ base path)
  plug Plug.Static,
    at: "/_",
    from: {:lazypock, "priv/static/studio"},
    gzip: not code_reloading?,
    raise_on_missing_only: code_reloading?

  # Serve global static files
  plug Plug.Static,
    at: "/",
    from: :lazypock,
    gzip: not code_reloading?,
    only: LazypockWeb.static_paths(),
    raise_on_missing_only: code_reloading?

  if code_reloading? do
    plug Phoenix.CodeReloader
    plug Phoenix.Ecto.CheckRepoStatus, otp_app: :lazypock
  end

  plug Plug.RequestId
  plug Plug.Telemetry, event_prefix: [:phoenix, :endpoint]

  plug Plug.Parsers,
    parsers: [:urlencoded, :multipart, :json],
    pass: ["*/*"],
    json_decoder: Phoenix.json_library()

  plug Plug.MethodOverride
  plug Plug.Head
  plug Plug.Session, @session_options
  # CORS: comma-separated origins from LAZYPOCK_CORS_ORIGINS (default: local dev origins).
  # e.g. LAZYPOCK_CORS_ORIGINS="https://app.example.com,http://localhost:5173"
  #      LAZYPOCK_CORS_ORIGINS="*" (allow all; requires LAZYPOCK_CORS_CREDENTIALS=0)
  cors_origins =
    System.get_env("LAZYPOCK_CORS_ORIGINS") || "http://localhost:4000,http://localhost:5173"

  # docker-compose may pass the value with literal quotes — strip them.
  cors_origins = String.trim(cors_origins, "\"")

  # Always allow the app's own origin (the lazypock-ts SDK derives its
  # realtime socket + API calls from baseUrl, so the browser sends the
  # API host as Origin).
  own_origin = "http://localhost:#{System.get_env("PORT", "4000")}"

  cors_credentials = System.get_env("LAZYPOCK_CORS_CREDENTIALS", "true") == "true"

  cors_plug_opts = [
    origin:
      [own_origin | (cors_origins |> String.split(",") |> Enum.map(&String.trim/1))]
      |> Enum.uniq()
  ]

  cors_plug_opts =
    if cors_credentials, do: Keyword.put(cors_plug_opts, :credentials, true), else: cors_plug_opts

  plug CORSPlug, cors_plug_opts
  plug LazypockWeb.Router
end
