# DDL + registry throughput benchmarks
#
# Prerequisites (from `core/`):
#
#     mix ecto.create && mix ecto.migrate
#
# Run:
#
#     mix bench.ddl
#
# These run real DDL against the database configured by `DATABASE_URL`
# (default: the local `lazypock_dev` database). Every iteration creates and
# drops a throwaway collection, so only run against a disposable/dev database.

alias Lazypock.Collections.Registry
alias Lazypock.Schema.DDL

fields = for i <- 1..10, do: %{"name" => "field_#{i}", "type" => "text"}

create_and_drop = fn ->
  name = "bench_#{System.unique_integer([:positive])}"
  {:ok, _} = DDL.create_collection(name, fields: fields)
  _ = DDL.drop_collection(name)
  :ok
end

IO.puts("\n== Serial: DDL vs ETS registry read ==\n")

Benchee.run(
  %{
    "create + drop collection (10 fields)" => create_and_drop,
    "Registry.get/1 (ETS read)" => fn -> Registry.get("users") end
  },
  warmup: 1,
  time: 5,
  memory_time: 1,
  formatters: [Benchee.Formatters.Console]
)

IO.puts("\n== Parallel x10: concurrent DDL throughput ==\n")

Benchee.run(
  %{"create + drop collection (10 fields)" => create_and_drop},
  parallel: 10,
  warmup: 1,
  time: 5,
  formatters: [Benchee.Formatters.Console]
)
