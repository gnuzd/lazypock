# Filter compiler benchmarks
#
# Run from `core/`:
#
#     mix bench.filter
#
# Pure (in-memory) — no database or network required.
#
# `FilterCompiler.compile/4` memoizes the parsed AST in ETS (see
# `Lazypock.Schemas.FilterCompiler`), so a "cold" run clears the cache and
# pays the tokenize+parse cost, while a "warm" run reuses the AST and only
# re-emits SQL. Comparing the two isolates the benefit of the memo cache.

alias Lazypock.Schemas.FilterCompiler

simple = "title = 'hello'"

complex =
  "(title ~ 'x' && views > 5) || (published = true && author = 'me' && rating >= 4)"

Benchee.run(
  %{
    "simple — cold cache (clear + compile)" => fn ->
      FilterCompiler.clear_cache()
      FilterCompiler.compile(simple)
    end,
    "simple — warm cache" => fn ->
      FilterCompiler.compile(simple)
    end,
    "complex — cold cache (clear + compile)" => fn ->
      FilterCompiler.clear_cache()
      FilterCompiler.compile(complex)
    end,
    "complex — warm cache" => fn ->
      FilterCompiler.compile(complex)
    end,
    "complex — warm cache + sort/list shape" => fn ->
      {:ok, {where, params}} = FilterCompiler.compile(complex)
      {:ok, {FilterCompiler.inline_params(where, params), []}}
    end
  },
  warmup: 1,
  time: 3,
  memory_time: 1,
  formatters: [Benchee.Formatters.Console]
)
