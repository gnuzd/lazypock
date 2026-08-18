defmodule LazypockWeb.CronControllerTest do
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Cron
  alias Lazypock.Repo
  alias Lazypock.Auth.SuperUser
  alias Lazypock.Auth.Token

  setup do
    {:ok, conn: build_conn()}
  end

  defp auth_conn(conn) do
    superuser = %SuperUser{
      id: Ecto.UUID.generate(),
      email: Ecto.UUID.generate() <> "@test.com",
      password_hash: Bcrypt.hash_pwd_salt("password")
    }

    Repo.insert!(superuser)
    {:ok, token} = Token.generate_access_token(superuser)
    put_req_header(conn, "authorization", "Bearer #{token}")
  end

  defp job_attrs(overrides \\ %{}) do
    Map.merge(
      %{
        "name" => "webhook",
        "expression" => "*/5 * * * *",
        "timezone" => "UTC",
        "enabled" => true,
        "action" => "http",
        "config" => %{"url" => "https://example.com/hook", "method" => "POST"}
      },
      overrides
    )
  end

  test "all cron endpoints require a superuser" do
    conn = get(build_conn(), "/api/crons")
    assert response(conn, 403)

    conn = post(build_conn(), "/api/crons", job_attrs())
    assert response(conn, 403)

    conn = post(build_conn(), "/api/crons/00000000-0000-0000-0000-000000000000")
    assert response(conn, 403)

    conn = delete(build_conn(), "/api/crons/00000000-0000-0000-0000-000000000000")
    assert response(conn, 403)
  end

  test "creates a cron job" do
    conn = post(auth_conn(build_conn()), "/api/crons", job_attrs())
    body = json_response(conn, 201)

    assert body["id"]
    assert body["name"] == "webhook"
    assert body["expression"] == "*/5 * * * *"
    assert body["action"] == "http"
    assert body["config"]["url"] == "https://example.com/hook"
    assert body["next_run_at"]
  end

  test "create validates expression and config" do
    conn = post(auth_conn(build_conn()), "/api/crons", job_attrs(%{"expression" => "bogus"}))
    assert json_response(conn, 400)["error"]

    conn =
      post(
        auth_conn(build_conn()),
        "/api/crons",
        job_attrs(%{"action" => "sql", "config" => %{}})
      )

    assert json_response(conn, 400)["error"]
  end

  test "lists cron jobs" do
    assert {:ok, _} = Cron.create(job_attrs(%{"name" => "a"}))
    assert {:ok, _} = Cron.create(job_attrs(%{"name" => "b"}))

    conn = get(auth_conn(build_conn()), "/api/crons")
    body = json_response(conn, 200)
    assert Enum.map(body["items"], & &1["name"]) == ["a", "b"]
  end

  test "shows a single job" do
    assert {:ok, job} = Cron.create(job_attrs())

    conn = get(auth_conn(build_conn()), "/api/crons/" <> job["id"])
    body = json_response(conn, 200)
    assert body["id"] == job["id"]

    conn = get(auth_conn(build_conn()), "/api/crons/00000000-0000-0000-0000-000000000000")
    assert response(conn, 404)
  end

  test "updates a job (patch)" do
    assert {:ok, job} = Cron.create(job_attrs(%{"enabled" => false}))

    conn = patch(auth_conn(build_conn()), "/api/crons/" <> job["id"], %{"enabled" => true})
    body = json_response(conn, 200)
    assert body["enabled"] == true
  end

  test "deletes a job" do
    assert {:ok, job} = Cron.create(job_attrs())

    conn = delete(auth_conn(build_conn()), "/api/crons/" <> job["id"])
    assert json_response(conn, 200)
    assert Cron.get(job["id"]) == nil
  end

  test "runs a job immediately (PocketBase parity: POST /api/crons/:id)" do
    assert {:ok, job} =
             Cron.create(
               job_attrs(%{"action" => "sql", "config" => %{"statement" => "SELECT 1"}})
             )

    conn = post(auth_conn(build_conn()), "/api/crons/" <> job["id"])
    body = json_response(conn, 200)
    assert body["last_status"] == "ok"
    assert body["last_duration_ms"] != nil

    # Unknown id → 404
    conn = post(auth_conn(build_conn()), "/api/crons/00000000-0000-0000-0000-000000000000")
    assert response(conn, 404)
  end

  test "validate endpoint returns next runs for a valid expression" do
    conn =
      post(auth_conn(build_conn()), "/api/crons/validate", %{
        "expression" => "0 9 * * *",
        "timezone" => "America/New_York"
      })

    body = json_response(conn, 200)
    assert body["valid"] == true
    assert length(body["nextRuns"]) == 5
  end

  test "validate endpoint rejects invalid expressions and timezones" do
    conn = post(auth_conn(build_conn()), "/api/crons/validate", %{"expression" => "nope"})
    assert json_response(conn, 200)["valid"] == false

    conn =
      post(auth_conn(build_conn()), "/api/crons/validate", %{
        "expression" => "* * * * *",
        "timezone" => "Not/AZone"
      })

    assert json_response(conn, 200)["valid"] == false
  end
end
