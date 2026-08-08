defmodule Lazypock.Hooks.Router do
  @moduledoc """
  Custom API route registration — the LazyPock equivalent of PocketBase's
  `routerAdd("GET", "/hello/{name}", (e) => e.json(200, ...))` inside the
  `onBeforeServe` hook.

  Users register routes inside an `on_before_serve` hook handler by putting
  `{method, path_pattern, handler}` tuples into the event's `:routes` list:

      defmodule MyApp.Hooks do
        use Lazypock.Hooks.Hook

        def on_before_serve(e) do
          routes = Lazypock.Hooks.Router.add(e, "GET", "/hello/{name}", fn ctx ->
            Lazypock.Hooks.Router.json(ctx, 200, %{"message" => "Hello \#{ctx.params["name"]}"})
          end)
          e = Lazypock.Hooks.Event.put(e, :routes, routes)
          Lazypock.Hooks.Event.next(e)
        end
      end

  ## Path patterns

  Paths use PocketBase/Go-ServeMux-style `{param}` segments:

    * `/hello/{name}` — matches one segment, captured as `ctx.params["name"]`
    * `/static/{path...}` — wildcard, matches one or more trailing segments
    * `/static/{$}` — matches exactly the path with optional trailing slash
    * `/hello` — exact match

  ## Handler context

  The handler receives a `ctx` map:

    * `ctx.conn` — the Phoenix `Plug.Conn`
    * `ctx.params` — path params (e.g. `%{"name" => "john"}`)
    * `ctx.query` — query params
    * `ctx.body` — parsed JSON body (if any)
    * `ctx.auth` — the authenticated user/superuser (or `nil`)
    * `ctx.app` — the app module

  Handlers return either a `%Plug.Conn{}` or a `{status, body}` tuple.

  ## Response helpers

    * `json(ctx, status, data)` — JSON response
    * `string(ctx, status, text)` — text/plain response
    * `html(ctx, status, html)` — text/html response
    * `no_content(ctx, status \\ 204)` — empty response
    * `redirect(ctx, status, location)` — redirect
  """

  alias Lazypock.Hooks.Event

  @type route :: {String.t(), String.t(), (map() -> Plug.Conn.t() | {integer(), term()})}

  @doc """
  Adds a custom route to the event's `:routes` list. Returns the updated list.
  """
  def add(%Event{} = e, method, path_pattern, handler) when is_function(handler, 1) do
    routes = Event.get(e, :routes) || []
    routes ++ [{String.upcase(method), path_pattern, handler}]
  end

  @doc "Returns all registered custom routes (fires `on_before_serve`)."
  def all do
    Lazypock.Hooks.App.trigger_before_serve(Lazypock.app())
  end

  @doc """
  Matches a request path against a route pattern.

  Returns `{:ok, params}` on match, `:error` otherwise.
  """
  def match(pattern, path) do
    do_match(split_segments(pattern), split_segments(path), %{})
  end

  # exact match with no segments left
  defp do_match([], [], params), do: {:ok, params}

  # trailing-slash wildcard: pattern "/static/" matches "/static/", "/static/a/b/c"
  defp do_match(["{$}"], [], params), do: {:ok, params}

  defp do_match([seg | pattern_rest], [path_seg | path_rest], params) do
    case seg do
      "{" <> _ = param ->
        if wildcard?(param) do
          {:ok, Map.put(params, param_name(param), Enum.join([path_seg | path_rest], "/"))}
        else
          do_match(pattern_rest, path_rest, Map.put(params, param_name(param), path_seg))
        end

      literal ->
        if literal == path_seg do
          do_match(pattern_rest, path_rest, params)
        else
          :error
        end
    end
  end

  defp do_match(_, _, _), do: :error

  defp split_segments(path) do
    path
    |> String.trim_leading("/")
    |> String.trim_trailing("/")
    |> String.split("/", trim: true)
  end

  defp param_name("{" <> rest) do
    rest
    |> String.trim_trailing("}")
    |> String.trim_trailing("...")
  end

  defp wildcard?(param), do: String.ends_with?(param, "...}")

  # ── Response helpers ────────────────────────────────────

  @doc "Sends a JSON response. Returns a `Plug.Conn`."
  def json(ctx, status, data) do
    conn = ctx.conn
    body = Jason.encode!(data)

    conn
    |> Plug.Conn.put_resp_content_type("application/json")
    |> Plug.Conn.send_resp(status, body)
  end

  @doc "Sends a plain-text response."
  def string(ctx, status, text) do
    ctx.conn
    |> Plug.Conn.put_resp_content_type("text/plain")
    |> Plug.Conn.send_resp(status, text)
  end

  @doc "Sends an HTML response."
  def html(ctx, status, html) do
    ctx.conn
    |> Plug.Conn.put_resp_content_type("text/html")
    |> Plug.Conn.send_resp(status, html)
  end

  @doc "Sends an empty response (default 204)."
  def no_content(ctx, status \\ 204) do
    ctx.conn
    |> Plug.Conn.put_resp_content_type("text/plain")
    |> Plug.Conn.send_resp(status, "")
  end

  @doc "Redirects to a location."
  def redirect(ctx, status, location) do
    ctx.conn
    |> Plug.Conn.put_resp_header("location", location)
    |> Plug.Conn.send_resp(status, "")
  end
end
