defmodule LazypockWeb.LogsControllerTest do
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Auth.SuperUser
  alias Lazypock.Auth.Token
  alias Lazypock.Logs
  alias Lazypock.Repo
  alias Lazypock.Settings

  defp auth_conn(conn) do
    superuser = %SuperUser{
      id: Ecto.UUID.generate(),
      email: "admin_logs_#{:erlang.unique_integer([:positive])}@test.com",
      password_hash: Bcrypt.hash_pwd_salt("password")
    }

    Repo.insert!(superuser)
    {:ok, token} = Token.generate_access_token(superuser)
    put_req_header(conn, "authorization", "Bearer #{token}")
  end

  defp insert_log(status, days_ago, collection) do
    Ecto.Adapters.SQL.query!(
      Repo,
      """
      INSERT INTO _request_logs (id, method, path, status, duration, ip, collection, created_at)
      VALUES (gen_random_uuid(), 'GET', '/api/x', $1, 5, '127.0.0.1', $2,
              now() - make_interval(days => $3))
      """,
      [status, collection, days_ago]
    )
  end

  defp listed_statuses(conn, query) do
    body = json_response(get(conn, "/api/logs?#{query}"), 200)
    body["items"] |> Enum.map(& &1["status"]) |> Enum.sort()
  end

  setup do
    Logs.clear_all()
    original = Settings.get()
    on_exit(fn -> Settings.put(original) end)
    :ok
  end

  test "requires superuser" do
    assert json_response(get(build_conn(), "/api/logs"), 403)
  end

  # The RequestLogger logs these very requests, so assertions are scoped to the
  # collection the test inserts (`logtest`) rather than counting every row.
  test "filters by status class" do
    insert_log(200, 0, "logtest")
    insert_log(404, 0, "logtest")
    insert_log(500, 0, "logtest")

    conn = auth_conn(build_conn())

    assert listed_statuses(conn, "collection=logtest&status=2xx") == [200]
    assert listed_statuses(conn, "collection=logtest&status=4xx") == [404]
    assert listed_statuses(conn, "collection=logtest&status=5xx") == [500]
    assert listed_statuses(conn, "collection=logtest") == [200, 404, 500]
  end

  test "filters by exact status and combines with the collection filter" do
    insert_log(404, 0, "posts")
    insert_log(404, 0, "users")
    insert_log(500, 0, "users")

    conn = auth_conn(build_conn())

    assert listed_statuses(conn, "status=404&collection=users") == [404]
    assert listed_statuses(conn, "collection=users") == [404, 500]
    assert listed_statuses(conn, "collection=posts") == [404]
  end

  test "retention endpoint round-trips and rejects junk" do
    conn = auth_conn(build_conn())

    assert json_response(get(conn, "/api/logs/retention"), 200)["days"] == nil

    assert json_response(put(conn, "/api/logs/retention", %{"days" => 30}), 200)["days"] == 30
    assert json_response(get(conn, "/api/logs/retention"), 200)["days"] == 30

    assert json_response(put(conn, "/api/logs/retention", %{"days" => 0}), 200)["days"] == nil
    assert json_response(put(conn, "/api/logs/retention", %{"days" => "nope"}), 400)
  end

  test "DELETE /api/logs?days=N removes only entries older than N days" do
    insert_log(200, 10, "logtest")
    insert_log(200, 1, "logtest")

    body = json_response(delete(auth_conn(build_conn()), "/api/logs?days=7"), 200)
    assert body["days"] == 7
    assert body["deleted"] == 1

    remaining =
      json_response(get(auth_conn(build_conn()), "/api/logs?collection=logtest"), 200)

    assert remaining["totalItems"] == 1
  end

  test "DELETE /api/logs?all=true clears everything" do
    insert_log(200, 0, "logtest")
    insert_log(500, 0, "logtest")

    body = json_response(delete(auth_conn(build_conn()), "/api/logs?all=true"), 200)
    assert body["all"] == true

    # The test's own `all=true` request is logged after the truncate, so count
    # only the rows the test inserted.
    remaining =
      json_response(get(auth_conn(build_conn()), "/api/logs?collection=logtest"), 200)

    assert remaining["totalItems"] == 0
  end
end
