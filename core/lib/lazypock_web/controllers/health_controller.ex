defmodule LazypockWeb.HealthController do
  use LazypockWeb, :controller

  def index(conn, _params) do
    json(conn, %{"status" => "ok", "version" => "0.1.0"})
  end
end
