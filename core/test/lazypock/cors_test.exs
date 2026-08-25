defmodule Lazypock.CORSTest do
  use LazypockWeb.ConnCase, async: false

  import Plug.Conn

  # The CORS plug runs in the endpoint, so exercise it through the real
  # router with an OPTIONS preflight from a cross-origin client.
  describe "preflight (OPTIONS)" do
    setup do
      # Allow any origin via the env override so the preflight path runs.
      previous = System.get_env("LAZYPOCK_CORS_ORIGINS")
      System.put_env("LAZYPOCK_CORS_ORIGINS", "*")
      Lazypock.CORS.refresh_origins()

      on_exit(fn ->
        if previous,
          do: System.put_env("LAZYPOCK_CORS_ORIGINS", previous),
          else: System.delete_env("LAZYPOCK_CORS_ORIGINS")

        Lazypock.CORS.refresh_origins()
      end)

      :ok
    end

    test "allows the custom X-Connection-Id header sent by the SDK" do
      conn =
        build_conn(:options, "/api/users/auth-with-password")
        |> put_req_header("origin", "http://localhost:5173")
        |> put_req_header("access-control-request-method", "POST")
        |> put_req_header("access-control-request-headers", "x-connection-id, content-type")
        |> LazypockWeb.Endpoint.call([])

      assert conn.status == 204

      assert get_resp_header(conn, "access-control-allow-headers") == [
               "x-connection-id, content-type"
             ]
    end

    test "echoes whatever headers the browser requested" do
      conn =
        build_conn(:options, "/api/users/auth-with-password")
        |> put_req_header("origin", "http://localhost:5173")
        |> put_req_header("access-control-request-method", "POST")
        |> put_req_header("access-control-request-headers", "authorization, x-connection-id")
        |> LazypockWeb.Endpoint.call([])

      assert conn.status == 204

      assert get_resp_header(conn, "access-control-allow-headers") == [
               "authorization, x-connection-id"
             ]

      assert get_resp_header(conn, "access-control-allow-methods") == [
               "GET, POST, PUT, PATCH, DELETE, OPTIONS"
             ]
    end
  end
end
