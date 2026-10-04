defmodule Lazypock.LogsTest do
  use LazypockWeb.ConnCase, async: false

  alias Lazypock.Logs
  alias Lazypock.Repo
  alias Lazypock.Settings

  setup do
    Logs.clear_all()
    original = Settings.get()
    on_exit(fn -> Settings.put(original) end)
    :ok
  end

  defp insert_log(days_ago) do
    Ecto.Adapters.SQL.query!(
      Repo,
      """
      INSERT INTO _request_logs (id, method, path, status, duration, created_at)
      VALUES (gen_random_uuid(), 'GET', '/x', 200, 1, now() - make_interval(days => $1))
      """,
      [days_ago]
    )
  end

  defp count do
    {:ok, %{rows: [[n]]}} =
      Ecto.Adapters.SQL.query(Repo, "SELECT COUNT(*) FROM _request_logs", [])

    n
  end

  test "retention defaults to nil and round-trips" do
    assert Logs.retention_days() == nil

    assert {:ok, 14} = Logs.put_retention_days(14)
    assert Logs.retention_days() == 14

    # A string from a form/query body works too.
    assert {:ok, 30} = Logs.put_retention_days("30")
    assert Logs.retention_days() == 30

    assert {:ok, nil} = Logs.put_retention_days(0)
    assert Logs.retention_days() == nil

    assert {:error, _} = Logs.put_retention_days(-1)
    assert {:error, _} = Logs.put_retention_days("nope")
  end

  test "clean_old is a no-op while retention is off" do
    insert_log(30)
    assert Logs.clean_old() == 0
    assert count() == 1
  end

  test "clean_old deletes past the retention; Cleaner.run delegates to it" do
    {:ok, 7} = Logs.put_retention_days(7)
    insert_log(30)
    insert_log(1)

    assert Lazypock.Logs.Cleaner.run() == 1
    assert count() == 1
  end

  test "delete_older_than is explicit and independent of the setting" do
    insert_log(10)
    insert_log(2)

    assert Logs.delete_older_than(5) == 1
    assert count() == 1
  end
end
