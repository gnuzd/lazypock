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
        body = capture_body(conn)

        # Production inserts asynchronously so the response is never blocked.
        # Under test (ExUnit loaded) we insert synchronously instead: the Ecto
        # sandbox hands DB connections to the test process only, and a spawned
        # task that outlives the test crashes with "Postgrex.Protocol
        # disconnected: owner exited" noise once the owner exits. The
        # request-logger tests poll for the row anyway, so a synchronous insert
        # is behaviorally identical there.
        if Code.ensure_loaded?(ExUnit) do
          log_request(conn, duration, body)
        else
          Task.start(fn ->
            log_request(conn, duration, body)
          end)
        end
      end

      conn
    end)
  end

  defp loggable?(conn) do
    path = conn.request_path
    String.starts_with?(path, "/api/") and not String.starts_with?(path, "/api/health")
  end

  defp log_request(conn, duration_us, body) do
    Ecto.Adapters.SQL.query!(
      Repo,
      """
      INSERT INTO _request_logs (method, path, status, duration, ip, user_agent, referer, collection, error, body)
      VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
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
        extract_error(conn),
        body
      ]
    )
  rescue
    _ -> :ok
  end

  # Capture the request payload (POST/PATCH/PUT) so log detail can show exactly
  # which fields were sent. JSON bodies are re-encoded with sensitive keys
  # redacted; other content types (multipart, raw) are summarized.
  defp capture_body(conn) do
    if conn.method in ~w(POST PATCH PUT) and Map.has_key?(conn.body_params, "_json") do
      conn.body_params["_json"]
      |> redact_params()
      |> Jason.encode!()
    else
      case conn.body_params do
        %Plug.Conn.Unfetched{} ->
          nil

        %{} = params when map_size(params) > 0 ->
          params
          |> redact_params()
          |> Jason.encode!()

        _ ->
          nil
      end
    end
  rescue
    _ -> nil
  end

  @sensitive_keys ~w(password passwordConfirm password_confirm oldPassword newPassword token verificationToken resetToken secret apiKey api_key)

  defp redact_params(params) when is_map(params) do
    Map.new(params, fn
      {key, value} ->
        {key, redact_value(key, value)}
    end)
  end

  defp redact_params(value) when is_list(value), do: Enum.map(value, &redact_params/1)
  defp redact_params(value), do: value

  defp redact_value(key, value) do
    if is_binary(key) and String.downcase(key) in @sensitive_keys do
      "[REDACTED]"
    else
      redact_params(value)
    end
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
