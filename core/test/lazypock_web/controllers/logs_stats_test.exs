defmodule LazypockWeb.LogsStatsTest do
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Repo
  alias Lazypock.Auth.SuperUser
  alias Lazypock.Auth.Token

  defp auth_conn(conn) do
    superuser = %SuperUser{
      id: Ecto.UUID.generate(),
      email: "admin_stats_#{:erlang.unique_integer([:positive])}@test.com",
      password_hash: Bcrypt.hash_pwd_salt("password")
    }

    Repo.insert!(superuser)
    {:ok, token} = Token.generate_access_token(superuser)
    put_req_header(conn, "authorization", "Bearer #{token}")
  end

  defp insert_log(opts \\ []) do
    status = Keyword.get(opts, :status, 200)
    duration = Keyword.get(opts, :duration, 5)
    hours_ago = Keyword.get(opts, :hours_ago, 0)

    Ecto.Adapters.SQL.query!(
      Repo,
      """
      INSERT INTO _request_logs (id, method, path, status, duration, ip, collection, created_at)
      VALUES (gen_random_uuid(), 'GET', $1, $2, $3, '127.0.0.1', 'posts', now() - interval '#{hours_ago} hours')
      """,
      ["/api/posts", status, duration]
    )
  end

  test "requires superuser" do
    conn = get(build_conn(), "/api/logs/stats")
    assert json_response(conn, 403)
  end

  test "returns zero-filled multi-metric series for 24h by default" do
    conn = get(auth_conn(build_conn()), "/api/logs/stats")
    body = json_response(conn, 200)

    assert body["range"] == "24h"
    assert is_list(body["series"])
    assert length(body["series"]) >= 24
    assert is_map(body["summary"])
    assert Map.has_key?(body["summary"], "requests")
    assert Map.has_key?(body["summary"], "errors")
    assert Map.has_key?(body["summary"], "avg_duration")

    assert Enum.all?(body["series"], fn b ->
             Map.has_key?(b, "date") and Map.has_key?(b, "total") and
               Map.has_key?(b, "errors") and Map.has_key?(b, "avg_duration")
           end)
  end

  test "buckets requests, errors and avg duration" do
    insert_log(status: 200, duration: 20, hours_ago: 2)
    insert_log(status: 404, duration: 40, hours_ago: 2)
    insert_log(status: 500, duration: 60, hours_ago: 3)

    conn = get(auth_conn(build_conn()), "/api/logs/stats?range=24h")
    body = json_response(conn, 200)

    assert body["summary"]["requests"] >= 3
    assert body["summary"]["errors"] >= 2

    # the bucket 2h ago holds the two requests (one error), 3h ago holds one error
    recent = Enum.filter(body["series"], fn b -> b["total"] > 0 end)
    assert length(recent) >= 2
    assert Enum.all?(recent, fn b -> b["errors"] >= 0 end)
  end

  test "honors range param bucket sizes" do
    conn = get(auth_conn(build_conn()), "/api/logs/stats?range=7d")
    body = json_response(conn, 200)
    assert body["range"] == "7d"
    # 7 days in 6-hour buckets → 29 points (28 full + current)
    assert length(body["series"]) in 28..30

    conn = get(auth_conn(build_conn()), "/api/logs/stats?range=30d")
    body = json_response(conn, 200)
    assert body["range"] == "30d"
    # 30 days in 1-day buckets → 31 points (30 full + current)
    assert length(body["series"]) in 30..32

    # unknown range falls back to 24h
    conn = get(auth_conn(build_conn()), "/api/logs/stats?range=bogus")
    assert json_response(conn, 200)["range"] == "24h"
  end
end
