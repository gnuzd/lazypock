defmodule LazypockWeb.Plugs.CustomRoutes do
  @moduledoc """
  Dispatches custom API routes registered via `Lazypock.Hooks.Router`
  (the PocketBase `routerAdd()` equivalent, registered inside the
  `onBeforeServe` hook).

  This plug runs early in the API pipeline. If a registered route matches
  the request method + path, the route's handler is invoked with a `ctx`
  map and its result (a `%Plug.Conn{}` or `{status, body}` tuple) is sent.

  If no custom route matches, the request proceeds unchanged down the
  regular router pipeline.
  """

  import Plug.Conn

  alias Lazypock.Hooks.Router

  def init(opts), do: opts

  def call(conn, _opts) do
    routes = Router.all()

    # Requests arrive under /api/... but routes are registered PB-style at the
    # app root (e.g. "/hello/{name}"), so strip the leading /api prefix.
    path =
      "/" <> Enum.join(conn.path_info, "/")
      |> String.replace_prefix("/api", "")
      |> then(&if(&1 == "", do: "/", else: &1))

    case find_route(routes, conn.method, path) do
      nil ->
        conn

      {_method, _pattern, handler, params} ->
        ctx = build_ctx(conn, params)
        invoke(handler, ctx) |> Plug.Conn.halt()
    end
  end

  # ── Route matching ──────────────────────────────────────

  defp find_route(routes, method, path) do
    Enum.find_value(routes, fn {route_method, pattern, handler} ->
      if route_method == String.upcase(method) do
        case Router.match(pattern, path) do
          {:ok, params} -> {route_method, pattern, handler, params}
          :error -> nil
        end
      end
    end)
  end

  defp build_ctx(conn, params) do
    %{
      conn: conn,
      params: params,
      query: conn.query_params,
      body: conn.body_params,
      auth: conn.assigns[:current_user] || conn.assigns[:current_superuser],
      app: Lazypock.app()
    }
  end

  defp invoke(handler, ctx) do
    result = handler.(ctx)
    conn = ctx.conn

    case result do
      %Plug.Conn{} = conn -> conn
      {status, body} when is_integer(status) -> send_simple(conn, status, body)
      _ -> send_simple(conn, 500, %{error: "Invalid custom route handler result"})
    end
  end

  defp send_simple(conn, status, body) when is_map(body) or is_list(body) do
    conn
    |> put_resp_content_type("application/json")
    |> send_resp(status, Jason.encode!(body))
  end

  defp send_simple(conn, status, body) when is_binary(body) do
    conn
    |> put_resp_content_type("text/plain")
    |> send_resp(status, body)
  end

  defp send_simple(conn, status, body) do
    conn
    |> put_resp_content_type("application/json")
    |> send_resp(status, Jason.encode!(%{result: body}))
  end
end
