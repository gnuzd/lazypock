defmodule CustomApiHooks do
  # NOTE: runtime-compiled user hook (LAZYPOCK_HOOKS_DIR) — the server compiles
  # this at boot; editor LSPs outside the mix project can't resolve Lazypock.*
  # modules, which is expected (see HOOKS_SECURITY.md).
  @moduledoc """
  Example custom API routes — the PocketBase `routerAdd()` equivalent.

  Routes registered here (inside `on_before_serve`) are reachable under
  `/api/...` on the running server, e.g. after `docker compose up`:

      GET /api/hello/chris   → {"message": "Hello chris!"}
      GET /api/example/time  → {"now": "2026-01-01T00:00:00Z"}

  See `Lazypock.Hooks.Router` for path patterns (`{param}`, `{path...}`,
  `{$}`) and response helpers (`json/3`, `string/3`, `html/3`, ...).
  """
  use Lazypock.Hooks.Hook

  alias Lazypock.Hooks.Event
  alias Lazypock.Hooks.Router

  # PocketBase: onBeforeServe + routerAdd — register custom API routes.
  def on_before_serve(%Event{} = e) do
    routes = [
      {"GET", "/hello/{name}", fn ctx ->
        Router.json(ctx, 200, %{"message" => "Hello #{ctx.params["name"]}!"})
      end},
      {"GET", "/example/time", fn ctx ->
        Router.json(ctx, 200, %{"now" => DateTime.utc_now()})
      end}
    ]

    e = Event.put(e, :routes, routes)
    Event.next(e)
  end
end
