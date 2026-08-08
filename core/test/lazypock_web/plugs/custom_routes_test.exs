defmodule LazypockWeb.Plugs.CustomRoutesTest do
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Hooks.{Registry, Event, Router}

  setup do
    Registry.clear()
    on_exit(fn -> Registry.clear() end)
    :ok
  end

  defp register_route(method, path, handler) do
    Registry.register(
      :on_before_serve,
      {:fun,
       fn e ->
         routes = Router.add(e, method, path, handler)
         Event.next(Event.put(e, :routes, routes))
       end}
    )
  end

  test "custom route returns JSON via Router.json/3" do
    register_route("GET", "/hello/{name}", fn ctx ->
      Router.json(ctx, 200, %{"message" => "Hello #{ctx.params["name"]}"})
    end)

    conn = get(build_conn(), "/api/hello/john")
    assert conn.status == 200
    assert json_response(conn, 200) == %{"message" => "Hello john"}
  end

  test "custom route matches query params and returns text" do
    register_route("POST", "/echo", fn ctx ->
      Router.string(ctx, 200, "q=#{ctx.query["q"]}")
    end)

    conn = post(build_conn(), "/api/echo?q=hello")
    assert conn.status == 200
    assert conn.resp_body == "q=hello"
  end

  test "unmatched custom route falls through to 404" do
    register_route("GET", "/hello/{name}", fn ctx ->
      Router.json(ctx, 200, %{"message" => "hi"})
    end)

    conn = get(build_conn(), "/api/unknown-route")
    assert conn.status == 404
  end
end
