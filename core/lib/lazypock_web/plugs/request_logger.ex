defmodule LazypockWeb.Plugs.RequestLogger do
  @moduledoc """
  Records API request metrics into the `_request_logs` table.

  Uses `register_before_send` to capture the response status
  and compute duration. Inserts asynchronously to avoid
  blocking the response.
  """

  import Plug.Conn

  alias Lazypock.Repo

  def init(opts), do: opts

  def call(conn, _opts) do
    start_time = System.monotonic_time(:microsecond)

    register_before_send(conn, fn conn ->
      duration = System.monotonic_time(:microsecond) - start_time

      if loggable?(conn) do
        Task.start(fn ->
          log_request(conn, duration)
        end)
      end

      conn
    end)
  end

  defp loggable?(conn) do
    path = conn.request_path
    String.starts_with?(path, "/api/") and not String.starts_with?(path, "/api/health")
  end

  defp log_request(conn, duration_us) do
    Ecto.Adapters.SQL.query!(
      Repo,
      """
      INSERT INTO _request_logs (method, path, status, duration, ip, user_agent, referer, collection, error)
      VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
      """,
      [
        conn.method,
        conn.request_path,
        conn.status,
        max(0, div(duration_us, 1000)),
        format_ip(conn.remote_ip),
        get_req_header(conn, "user-agent") |> List.first(),
        get_req_header(conn, "referer") |> List.first(),
        extract_collection(conn),
        extract_error(conn)
      ]
    )
  rescue
    _ -> :ok
  end

  # Extract user-facing collection name from path
  defp extract_collection(conn) do
    path_segments = String.split(conn.request_path, "/")

    case path_segments do
      ["", "api", coll | _rest] when coll != "" ->
        known_api = ~w(collections superusers files logs settings)
        if coll in known_api, do: nil, else: coll

      _ ->
        nil
    end
  end

  defp extract_error(conn) do
    case conn.assigns[:error_message] do
      nil -> nil
      msg when is_binary(msg) -> msg
      _ -> nil
    end
  end

  defp format_ip({a, b, c, d}), do: "#{a}.#{b}.#{c}.#{d}"
  defp format_ip(ip) when is_tuple(ip), do: ip |> :inet.ntoa() |> List.to_string()
  defp format_ip(_), do: nil
end
