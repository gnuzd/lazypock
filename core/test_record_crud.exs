# Run directly: elixir -r test_record_crud.exs
Mix.install([:decimal])
{:ok, _} = Application.ensure_all_started(:postgrex)
{:ok, _} = Application.ensure_all_started(:ecto)
{:ok, _} = Application.ensure_all_started(:ecto_sql)

# Just compile and run tests
Code.compile_file("test/support/test_helpers.ex")

IO.puts("Testing record CRUD via DynamicController...")
IO.puts("(Run `mix test test/lazypock_web/controllers/dynamic_controller_test.exs` instead)")
