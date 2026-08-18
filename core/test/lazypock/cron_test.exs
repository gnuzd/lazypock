defmodule Lazypock.CronTest do
  use ExUnit.Case, async: false

  alias Lazypock.Cron
  alias Lazypock.Hooks.Event
  alias Lazypock.Hooks.Registry
  alias Lazypock.Repo

  setup do
    Ecto.Adapters.SQL.Sandbox.checkout(Lazypock.Repo)
    on_exit(fn -> Ecto.Adapters.SQL.Sandbox.checkin(Lazypock.Repo) end)
    :ok
  end

  # ── Expression validation ─────────────────────────

  describe "parse_expression/1" do
    test "accepts standard 5-field expressions" do
      assert {:ok, _} = Cron.parse_expression("*/5 * * * *")
      assert {:ok, _} = Cron.parse_expression("0 0 * * 1-5")
      assert {:ok, _} = Cron.parse_expression("15 6,18 * * *")
    end

    test "accepts 6-field (PocketBase-style, with seconds)" do
      assert {:ok, _} = Cron.parse_expression("0 */5 * * * *")
      assert {:ok, _} = Cron.parse_expression("30 0 2 * * *")
    end

    test "accepts @-macros but not @reboot" do
      assert {:ok, _} = Cron.parse_expression("@daily")
      assert {:ok, _} = Cron.parse_expression("@hourly")
      assert {:ok, _} = Cron.parse_expression("@weekly")
      assert {:error, msg} = Cron.parse_expression("@reboot")
      assert msg =~ "not supported"
    end

    test "rejects malformed expressions" do
      assert {:error, _} = Cron.parse_expression(nil)
      assert {:error, _} = Cron.parse_expression("")
      assert {:error, _} = Cron.parse_expression("   ")
      assert {:error, _} = Cron.parse_expression("* * *")
      assert {:error, _} = Cron.parse_expression("* * * * * * *")
      assert {:error, _} = Cron.parse_expression("foo bar baz qux quux")
      assert {:error, _} = Cron.parse_expression("99 * * * *")
      assert {:error, _} = Cron.parse_expression("0 0 32 * *")
    end
  end

  # ── Timezone validation ────────────────────────────

  describe "valid_timezone?/1" do
    test "accepts known IANA zones and rejects unknown ones" do
      assert Cron.valid_timezone?("UTC")
      assert Cron.valid_timezone?("America/New_York")
      assert Cron.valid_timezone?("Asia/Tokyo")
      refute Cron.valid_timezone?("Not/AZone")
      refute Cron.valid_timezone?(nil)
      refute Cron.valid_timezone?(123)
    end
  end

  # ── Next-run computation ───────────────────────────

  describe "next_run_dates/4" do
    # ~U produces time_zone "Etc/UTC" while shift_zone!(dt, "UTC") yields
    # time_zone "UTC" — same instant, different struct. Compare instants.
    defp same_instant?(a, b), do: DateTime.compare(a, b) == :eq

    test "computes next runs in UTC" do
      from = ~U[2026-01-01 00:00:00Z]

      runs = Cron.next_run_dates("*/15 * * * *", "UTC", 3, from)
      assert length(runs) == 3
      assert same_instant?(Enum.at(runs, 0), ~U[2026-01-01 00:15:00Z])
      assert same_instant?(Enum.at(runs, 1), ~U[2026-01-01 00:30:00Z])
      assert same_instant?(Enum.at(runs, 2), ~U[2026-01-01 00:45:00Z])
    end

    test "honors the job timezone (wall-clock context)" do
      # 09:00 in New York (UTC-5 in January) = 14:00 UTC
      from = ~U[2026-01-15 00:00:00Z]
      utc_runs = Cron.next_run_dates("0 9 * * *", "UTC", 1, from)
      ny_runs = Cron.next_run_dates("0 9 * * *", "America/New_York", 1, from)
      assert same_instant?(List.first(utc_runs), ~U[2026-01-15 09:00:00Z])
      assert same_instant?(List.first(ny_runs), ~U[2026-01-15 14:00:00Z])
    end

    test "returns [] for invalid expressions or timezones" do
      assert Cron.next_run_dates("not a cron", "UTC") == []
      assert Cron.next_run_dates("*/5 * * * *", "Not/AZone") == []
    end
  end

  # ── CRUD ───────────────────────────────────────────

  describe "create/update/delete" do
    test "creates an enabled job with a computed next_run_at" do
      assert {:ok, job} =
               Cron.create(%{
                 "name" => "cleanup",
                 "expression" => "0 3 * * *",
                 "timezone" => "America/New_York",
                 "action" => "sql",
                 "config" => %{"statement" => "SELECT 1"}
               })

      assert job["id"]
      assert job["name"] == "cleanup"
      assert job["enabled"] == true
      assert job["action"] == "sql"
      assert job["config"]["statement"] == "SELECT 1"
      assert %DateTime{} = job["next_run_at"]
      assert job["last_status"] == nil
    end

    test "defaults: timezone UTC, action http, enabled true" do
      assert {:ok, job} =
               Cron.create(%{
                 "name" => "defaults",
                 "expression" => "*/5 * * * *",
                 "config" => %{"url" => "https://example.com/hook"}
               })

      assert job["timezone"] == "UTC"
      assert job["action"] == "http"
      assert job["enabled"] == true
    end

    test "rejects duplicate names" do
      attrs = %{
        "name" => "dup",
        "expression" => "* * * * *",
        "config" => %{"url" => "https://example.com"}
      }

      assert {:ok, _} = Cron.create(attrs)
      assert {:error, msg} = Cron.create(attrs)
      assert msg =~ "already exists"
    end

    test "rejects invalid expressions, timezones and configs" do
      assert {:error, _} = Cron.create(%{"name" => "x", "expression" => "bogus"})

      assert {:error, _} =
               Cron.create(%{
                 "name" => "x",
                 "expression" => "* * * * *",
                 "timezone" => "Mars/Olympus"
               })

      assert {:error, _} =
               Cron.create(%{
                 "name" => "x",
                 "expression" => "* * * * *",
                 "action" => "http",
                 "config" => %{}
               })

      assert {:error, _} =
               Cron.create(%{
                 "name" => "x",
                 "expression" => "* * * * *",
                 "action" => "sql",
                 "config" => %{}
               })

      assert {:error, _} =
               Cron.create(%{"name" => "x", "expression" => "* * * * *", "action" => "explode"})
    end

    test "disabled jobs have no next_run_at" do
      assert {:ok, job} =
               Cron.create(%{
                 "name" => "paused",
                 "expression" => "* * * * *",
                 "enabled" => false,
                 "config" => %{"url" => "https://example.com"}
               })

      assert job["enabled"] == false
      assert job["next_run_at"] == nil
    end

    test "update toggles enabled and recomputes next_run_at" do
      assert {:ok, job} =
               Cron.create(%{
                 "name" => "toggle",
                 "expression" => "* * * * *",
                 "enabled" => false,
                 "config" => %{"url" => "https://example.com"}
               })

      assert {:ok, updated} = Cron.update(job["id"], %{"enabled" => true})
      assert updated["enabled"] == true
      assert %DateTime{} = updated["next_run_at"]
    end

    test "update with unknown id returns :not_found" do
      assert {:error, :not_found} = Cron.update(Ecto.UUID.generate(), %{"enabled" => false})
    end

    test "delete removes the job" do
      assert {:ok, job} =
               Cron.create(%{
                 "name" => "gone",
                 "expression" => "* * * * *",
                 "config" => %{"url" => "https://example.com"}
               })

      assert Cron.delete(job["id"]) == :ok
      assert Cron.get(job["id"]) == nil
      assert Cron.delete(job["id"]) == :error
    end
  end

  # ── HTTP action ────────────────────────────────────

  describe "http action" do
    # Tiny one-shot HTTP server: accepts a single connection, returns the
    # canned response, closes. Runs outside the sandbox (no DB access).
    defp start_http_server(response) do
      {:ok, listen} =
        :gen_tcp.listen(0, [:binary, active: false, reuseaddr: true, ip: {127, 0, 0, 1}])

      {:ok, {_addr, port}} = :inet.sockname(listen)

      _ =
        Task.start(fn ->
          case :gen_tcp.accept(listen) do
            {:ok, socket} ->
              {:ok, _request} = :gen_tcp.recv(socket, 0, 5_000)
              :gen_tcp.send(socket, response)
              :gen_tcp.close(socket)

            _ ->
              :ok
          end
        end)

      port
    end

    test "GET succeeds and records an ok run" do
      port = start_http_server("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK")

      assert {:ok, job} =
               Cron.create(%{
                 "name" => "webhook-ok",
                 "expression" => "0 0 1 1 *",
                 "action" => "http",
                 "config" => %{"url" => "http://127.0.0.1:#{port}/hook", "method" => "GET"}
               })

      assert {:ok, updated} = Cron.run_now(job["id"])
      assert updated["last_status"] == "ok"
      assert updated["last_error"] == nil
    end

    test "POST with a body succeeds" do
      port = start_http_server("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")

      assert {:ok, job} =
               Cron.create(%{
                 "name" => "webhook-post",
                 "expression" => "0 0 1 1 *",
                 "action" => "http",
                 "config" => %{
                   "url" => "http://127.0.0.1:#{port}/hook",
                   "method" => "POST",
                   "headers" => %{"content-type" => "application/json"},
                   "body" => "{\"hello\": \"world\"}"
                 }
               })

      assert {:ok, updated} = Cron.run_now(job["id"])
      assert updated["last_status"] == "ok"
    end

    test "non-2xx response records an error" do
      port = start_http_server("HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n")

      assert {:ok, job} =
               Cron.create(%{
                 "name" => "webhook-500",
                 "expression" => "0 0 1 1 *",
                 "action" => "http",
                 "config" => %{"url" => "http://127.0.0.1:#{port}/hook"}
               })

      assert {:ok, updated} = Cron.run_now(job["id"])
      assert updated["last_status"] == "error"
      assert updated["last_error"] =~ "HTTP 500"
    end

    test "connection failure records an error" do
      # Reserve a port then close it, so the connect is refused.
      {:ok, listen} =
        :gen_tcp.listen(0, [:binary, active: false, reuseaddr: true, ip: {127, 0, 0, 1}])

      {:ok, {_addr, port}} = :inet.sockname(listen)
      :gen_tcp.close(listen)

      assert {:ok, job} =
               Cron.create(%{
                 "name" => "webhook-down",
                 "expression" => "0 0 1 1 *",
                 "action" => "http",
                 "config" => %{"url" => "http://127.0.0.1:#{port}/hook"}
               })

      assert {:ok, updated} = Cron.run_now(job["id"])
      assert updated["last_status"] == "error"
      assert is_binary(updated["last_error"])
    end
  end

  # ── Run now ────────────────────────────────────────

  describe "run_now/1" do
    setup do
      # Table lives inside the sandbox transaction — DDL is rolled back with
      # the test, so no explicit cleanup is needed (an on_exit DROP would run
      # from a process without sandbox ownership).
      Repo.query!("CREATE TABLE IF NOT EXISTS _cron_test_rows (id serial PRIMARY KEY, note text)")
      Repo.query!("TRUNCATE _cron_test_rows")
      :ok
    end

    test "sql action succeeds and records last run info" do
      assert {:ok, job} =
               Cron.create(%{
                 "name" => "sqljob",
                 "expression" => "0 0 1 1 *",
                 "action" => "sql",
                 "config" => %{"statement" => "INSERT INTO _cron_test_rows (note) VALUES ('hi')"}
               })

      assert {:ok, updated} = Cron.run_now(job["id"])
      assert updated["last_status"] == "ok"
      assert %DateTime{} = updated["last_run_at"]
      assert is_integer(updated["last_duration_ms"])

      {:ok, %{rows: [[count]]}} =
        Repo.query("SELECT COUNT(*) FROM _cron_test_rows")

      assert count == 1
    end

    test "failing sql records an error" do
      assert {:ok, job} =
               Cron.create(%{
                 "name" => "badjob",
                 "expression" => "0 0 1 1 *",
                 "action" => "sql",
                 "config" => %{"statement" => "SELECT * FROM _table_that_does_not_exist"}
               })

      assert {:ok, updated} = Cron.run_now(job["id"])
      assert updated["last_status"] == "error"
      assert is_binary(updated["last_error"])
    end

    test "hook action dispatches on_cron to registered handlers" do
      test_pid = self()

      Registry.clear()
      on_exit(fn -> Registry.clear() end)

      Lazypock.Hooks.Cron.on_cron(fn e ->
        send(test_pid, {:cron_fired, Event.get(e, :job)["name"]})
        Event.next(e)
      end)

      assert {:ok, job} =
               Cron.create(%{
                 "name" => "hookjob",
                 "expression" => "0 0 1 1 *",
                 "action" => "hook",
                 "config" => %{"event" => "custom"}
               })

      assert {:ok, updated} = Cron.run_now(job["id"])
      assert updated["last_status"] == "ok"
      assert_receive {:cron_fired, "hookjob"}, 1_000
    end

    test "unknown id returns :not_found" do
      assert {:error, :not_found} = Cron.run_now(Ecto.UUID.generate())
    end
  end
end
