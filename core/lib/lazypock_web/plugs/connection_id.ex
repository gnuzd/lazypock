defmodule LazypockWeb.Plugs.ConnectionId do
  @moduledoc """
  Reads the client-provided `X-Connection-Id` header and assigns it to
  `conn.assigns[:connection_id]`.

  The connection id identifies the socket connection (browser tab/device)
  that issued an HTTP request. Controllers pass it to
  `Lazypock.Realtime.Broadcaster` so realtime events can exclude the
  originating connection (while still reaching the same user's other
  connections). Opaque, length-capped; `nil` when absent.
  """

  import Plug.Conn

  @behaviour Plug

  @impl true
  def init(opts), do: opts

  @impl true
  def call(conn, _opts) do
    case get_req_header(conn, "x-connection-id") do
      [id | _] when is_binary(id) and id != "" ->
        assign(conn, :connection_id, String.slice(id, 0, 128))

      _ ->
        assign(conn, :connection_id, nil)
    end
  end
end
