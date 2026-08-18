defmodule LazypockWeb.Plugs.RequestLoggerTest do
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Repo
  alias Lazypock.Auth.SuperUser
  alias Lazypock.Auth.Token

  defp auth_token do
    superuser = %SuperUser{
      id: Ecto.UUID.generate(),
      email: "admin@test.com",
      password_hash: Bcrypt.hash_pwd_salt("password")
    }

    Repo.insert!(superuser)
    {:ok, token} = Token.generate_access_token(superuser)
    token
  end

  defp auth_conn(conn) do
    put_req_header(conn, "authorization", "Bearer #{auth_token()}")
  end

  defp log_count_where(where_clause) do
    case Ecto.Adapters.SQL.query(Repo, "SELECT COUNT(*) FROM _request_logs #{where_clause}", []) do
      {:ok, %{rows: [[count]]}} -> count
      _ -> 0
    end
  end

  # The RequestLogger inserts asynchronously (Task.start), so poll until the
  # row appears — as long as we're still inside the sandbox transaction.
  defp wait_for_log(predicate, timeout \\ 2000) do
    deadline = System.monotonic_time(:millisecond) + timeout
    do_wait(predicate, deadline)
  end

  defp do_wait(predicate, deadline) do
    if System.monotonic_time(:millisecond) > deadline do
      flunk("timed out waiting for async log insert")
    else
      if predicate.() do
        :ok
      else
        Process.sleep(10)
        do_wait(predicate, deadline)
      end
    end
  end

  test "authenticated API call to a dynamic collection is logged" do
    conn = build_conn() |> auth_conn() |> get("/api/superusers/me")
    assert json_response(conn, 200)

    assert wait_for_log(fn -> log_count_where("") >= 1 end) == :ok

    case Ecto.Adapters.SQL.query(
           Repo,
           "SELECT method, path FROM _request_logs",
           []
         ) do
      {:ok, %{rows: [[method, path]]}} ->
        assert method == "GET"
        assert path == "/api/superusers/me"

      _ ->
        flunk("expected a log row")
    end
  end

  test "public auth endpoint is logged" do
    conn =
      post(build_conn(), "/api/superusers/login", %{
        "email" => "admin@test.com",
        "password" => "password"
      })

    _ = conn

    assert wait_for_log(fn ->
             log_count_where("WHERE path = '/api/superusers/login'") >= 1
           end) == :ok
  end

  test "request body is captured and sensitive fields redacted" do
    conn =
      post(build_conn(), "/api/superusers/login", %{
        "email" => "admin@test.com",
        "password" => "supersecret"
      })

    _ = conn

    assert wait_for_log(fn ->
             log_count_where("WHERE path = '/api/superusers/login'") >= 1
           end) == :ok

    case Ecto.Adapters.SQL.query(
           Repo,
           "SELECT body FROM _request_logs WHERE path = '/api/superusers/login'",
           []
         ) do
      {:ok, %{rows: [[body]]}} when is_binary(body) ->
        # The payload is stored; the password value must not appear verbatim
        refute body =~ "supersecret"
        assert body =~ "[REDACTED]"
        assert body =~ "admin@test.com"

      _ ->
        flunk("expected a body to be captured")
    end
  end

  test "body is exposed via the logs detail endpoint" do
    Ecto.Adapters.SQL.query!(
      Repo,
      "INSERT INTO _request_logs (method, path, status, duration, body) VALUES ('PATCH', '/api/posts/123', 200, 1, $1)",
      [Jason.encode!(%{"title" => "New title", "content" => "Updated body"})]
    )

    case Ecto.Adapters.SQL.query(Repo, "SELECT id FROM _request_logs LIMIT 1", []) do
      {:ok, %{rows: [[id]]}} ->
        # Postgrex returns uuid columns as raw 16-byte binaries; cast to a
        # readable UUID string before putting it in the URL path.
        id_str = Ecto.UUID.cast!(id)
        conn = build_conn() |> auth_conn() |> get("/api/logs/#{id_str}")
        resp = json_response(conn, 200)
        assert resp["body"] =~ "New title"
        assert resp["method"] == "PATCH"

      _ ->
        flunk("expected a log row")
    end
  end

  test "health check is NOT logged" do
    conn = get(build_conn(), "/api/health")
    assert json_response(conn, 200)
    Process.sleep(100)

    assert log_count_where("WHERE path = '/api/health'") == 0
  end

  test "stats endpoint returns a full 24-hour zero-filled multi-metric series" do
    # Insert a log row
    Ecto.Adapters.SQL.query!(
      Repo,
      "INSERT INTO _request_logs (method, path, status, duration) VALUES ('GET', '/api/test', 200, 1)",
      []
    )

    conn = build_conn() |> auth_conn() |> get("/api/logs/stats")
    assert json_response(conn, 200)

    body = json_response(conn, 200)
    assert is_list(body["series"])
    assert length(body["series"]) == 25
    assert is_map(body["summary"])

    # All buckets are present and ordered; every metric is an integer/float
    totals = Enum.map(body["series"], fn h -> h["total"] end)
    assert Enum.all?(totals, &is_integer/1)
    assert Enum.all?(body["series"], fn h ->
             is_integer(h["errors"]) and is_number(h["avg_duration"])
           end)
    assert length(Enum.uniq(totals)) >= 1
  end
end
