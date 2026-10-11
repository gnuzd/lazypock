defmodule Lazypock.Schemas.FilterCompilerCacheTest do
  use ExUnit.Case, async: true

  alias Lazypock.Schemas.FilterCompiler
  alias Lazypock.Schemas.FilterCompiler.Cache

  @table :lazypock_filter_ast_cache

  test "the AST table is owned by the supervised cache process" do
    owner = Process.whereis(Cache)

    assert is_pid(owner),
           "the cache process must run so the ETS table has an owner that outlives callers"

    assert :ets.info(@table, :owner) == owner
  end

  test "the table outlives the short-lived process that populated it" do
    parent = self()

    {pid, ref} =
      spawn_monitor(fn ->
        assert {:ok, _} = FilterCompiler.compile("survives = 'caller'")
        send(parent, :compiled)
      end)

    assert_receive :compiled
    assert_receive {:DOWN, ^ref, :process, ^pid, :normal}

    # A caller-owned table (the old behaviour) is destroyed with its creator —
    # exactly what made concurrent compiles fail with
    # "the table identifier does not refer to an existing ETS table".
    assert :ets.info(@table, :owner) == Process.whereis(Cache)
    assert {:ok, {_sql, _params}} = FilterCompiler.compile("survives = 'caller'")
  end

  test "concurrent compiles from short-lived processes do not lose the table" do
    filters = Enum.map(1..25, &"concurrent#{&1} = #{&1}")

    results =
      filters
      |> Task.async_stream(&FilterCompiler.compile/1, max_concurrency: 8)
      |> Enum.map(fn {:ok, result} -> result end)

    assert Enum.all?(results, &match?({:ok, {_sql, _params}}, &1))
    assert :ets.info(@table, :owner) == Process.whereis(Cache)
  end
end
