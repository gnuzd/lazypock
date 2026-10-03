defmodule Lazypock.Files.LimiterTest do
  use ExUnit.Case, async: true

  alias Lazypock.Files.Limiter

  defp start_limiter!(limit, opts \\ []) do
    name = :"limiter_test_#{System.unique_integer([:positive])}"
    start_supervised!({Limiter, [name: name, limit: limit, wait: 2_000] ++ opts})
    name
  end

  test "never runs more than the configured number of jobs at once" do
    server = start_limiter!(2)
    {:ok, counter} = Agent.start_link(fn -> {0, 0} end)

    tasks =
      for _ <- 1..12 do
        Task.async(fn ->
          Limiter.run(
            fn ->
              Agent.update(counter, fn {cur, max} -> {cur + 1, max(max, cur + 1)} end)
              Process.sleep(30)
              Agent.update(counter, fn {cur, max} -> {cur - 1, max} end)
            end,
            server: server
          )
        end)
      end

    assert Enum.all?(Task.await_many(tasks, 10_000), &match?({:ok, _}, &1))
    assert Agent.get(counter, fn {_cur, max} -> max end) == 2
  end

  test "returns :overloaded instead of waiting forever when all slots are busy" do
    server = start_limiter!(1)
    parent = self()

    blocker =
      Task.async(fn ->
        Limiter.run(
          fn ->
            send(parent, :holding)

            receive do
              :release -> :ok
            after
              5_000 -> :ok
            end
          end,
          server: server
        )
      end)

    assert_receive :holding, 1_000

    assert {:error, :overloaded} = Limiter.run(fn -> :never end, server: server, wait: 50)

    send(blocker.pid, :release)
    assert {:ok, :ok} = Task.await(blocker, 5_000)
  end

  test "queued work runs as soon as a slot frees up" do
    server = start_limiter!(1)
    parent = self()

    first =
      Task.async(fn ->
        Limiter.run(
          fn ->
            send(parent, :first_started)
            Process.sleep(100)
            :first
          end,
          server: server
        )
      end)

    assert_receive :first_started, 1_000
    assert {:ok, :second} = Limiter.run(fn -> :second end, server: server, wait: 2_000)
    assert {:ok, :first} = Task.await(first, 5_000)
  end

  test "releases the slot even when the job raises" do
    server = start_limiter!(1)

    assert_raise RuntimeError, fn ->
      Limiter.run(fn -> raise "boom" end, server: server)
    end

    assert {:ok, :after} = Limiter.run(fn -> :after end, server: server, wait: 1_000)
  end

  test "stats/1 reports the running and queued counts" do
    server = start_limiter!(3)
    assert %{limit: 3, running: 0, queued: 0} = Limiter.stats(server)
  end

  test "a waiting caller that disconnects does not leak a queue slot" do
    server = start_limiter!(1)
    parent = self()

    holder =
      Task.async(fn ->
        Limiter.run(
          fn ->
            send(parent, :holding)

            receive do
              :release -> :ok
            after
              5_000 -> :ok
            end
          end,
          server: server
        )
      end)

    assert_receive :holding, 1_000

    # Start a waiter, then kill it while it is queued.
    waiter = Task.async(fn -> Limiter.run(fn -> :never end, server: server, wait: 5_000) end)
    Process.sleep(50)
    Task.shutdown(waiter, :brutal_kill)

    # Killing the waiter must not have consumed the slot; a new caller can wait.
    send(holder.pid, :release)
    assert {:ok, :ok} = Task.await(holder, 5_000)
    assert {:ok, :after} = Limiter.run(fn -> :after end, server: server, wait: 1_000)
  end
end
