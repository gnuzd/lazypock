defmodule Lazypock.Hooks.RouterTest do
  use ExUnit.Case, async: true

  alias Lazypock.Hooks.{Event, Router}

  describe "Router.match/2" do
    test "exact match" do
      assert {:ok, %{}} = Router.match("/hello", "/hello")
      assert :error = Router.match("/hello", "/hello/world")
    end

    test "single {param} segment" do
      assert {:ok, %{"name" => "john"}} = Router.match("/hello/{name}", "/hello/john")
      assert :error = Router.match("/hello/{name}", "/hello/john/extra")
    end

    test "wildcard {path...} matches trailing segments" do
      assert {:ok, %{"path" => "a/b/c"}} = Router.match("/static/{path...}", "/static/a/b/c")
      assert {:ok, %{"path" => "a"}} = Router.match("/static/{path...}", "/static/a")
    end

    test "trailing slash is normalized" do
      assert {:ok, %{"name" => "john"}} = Router.match("/hello/{name}", "/hello/john/")
    end
  end

  describe "Router.add/4" do
    test "appends to the event's :routes list with uppercased method" do
      e = %Event{event: :on_before_serve}
      e = Event.put(e, :routes, Router.add(e, "get", "/hello", fn ctx -> {:ok, ctx} end))
      e = Event.put(e, :routes, Router.add(e, "POST", "/bye", fn ctx -> {:ok, ctx} end))

      routes = Event.get(e, :routes)
      assert length(routes) == 2
      assert Enum.any?(routes, fn {m, p, _} -> m == "GET" and p == "/hello" end)
      assert Enum.any?(routes, fn {m, p, _} -> m == "POST" and p == "/bye" end)
    end
  end

  describe "Router response helpers" do
    import Plug.Test

    test "json/3 sends a JSON response" do
      conn = conn(:get, "/")
      conn = Router.json(%{conn: conn}, 200, %{"message" => "hi"})
      assert conn.status == 200
      assert conn.resp_body == ~s({"message":"hi"})

      assert Plug.Conn.get_resp_header(conn, "content-type") == [
               "application/json; charset=utf-8"
             ]
    end

    test "string/3 sends a text response" do
      conn = conn(:get, "/")
      conn = Router.string(%{conn: conn}, 201, "created")
      assert conn.status == 201
      assert conn.resp_body == "created"
    end

    test "no_content/2 sends empty 204" do
      conn = conn(:get, "/")
      conn = Router.no_content(%{conn: conn})
      assert conn.status == 204
      assert conn.resp_body == ""
    end

    test "redirect/3 sets location header" do
      conn = conn(:get, "/")
      conn = Router.redirect(%{conn: conn}, 302, "/login")
      assert conn.status == 302
      assert Plug.Conn.get_resp_header(conn, "location") == ["/login"]
    end
  end
end
