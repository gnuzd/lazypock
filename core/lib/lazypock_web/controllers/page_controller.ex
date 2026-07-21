defmodule LazypockWeb.PageController do
  use LazypockWeb, :controller

  def home(conn, _params) do
    render(conn, :home)
  end
end
