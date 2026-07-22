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

  # Serve static files (including the SPA)
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
  plug LazypockWeb.Router
end
