defmodule LazypockWeb.HealthController do
  use LazypockWeb, :controller

  def index(conn, _params) do
    version = Application.spec(:lazypock, :vsn) |> to_string()

    json(conn, %{"status" => "ok", "version" => version})
  end
end
