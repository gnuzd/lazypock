defmodule Lazypock.Schemas.FilterCompiler.Cache do
  @moduledoc """
  Owns the parsed-filter AST ETS table used by `Lazypock.Schemas.FilterCompiler`.

  Parsing (tokenize + parse) is the expensive part of compiling a filter and,
  unlike emission, depends on **nothing but the filter string** — no schema, no
  token values, no opts. Caching the AST therefore needs no invalidation when
  collections change, and relation dot-paths are still resolved against the
  live registry on every `FilterCompiler.compile/4` call.

  The table is created by this supervised process rather than by whichever
  request (or test) process happens to compile a filter first. An ETS table is
  owned by its creator and destroyed when that process exits, so a
  caller-owned table vanished at the end of the first request that populated
  it — dropping the cache and crashing any concurrent caller that was between
  its lookup and its insert.

  The table is `:public` so every caller writes to it directly (no GenServer
  round-trip), and `FilterCompiler` treats a missing table as a cache miss, so
  compilation still works if this process is down or restarting.
  """

  use GenServer

  @table :lazypock_filter_ast_cache
  # Hard cap on memoized filters. Reached only by a very diverse stream of
  # distinct filters; parses are cheap to redo, so the cache is emptied
  # wholesale at the cap instead of tracking LRU order.
  @limit 1_000

  def start_link(opts \\ []) do
    GenServer.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @doc """
  Look up the memoized AST for `filter_str`.

  Returns `{:ok, ast}` or `:miss` — including when the table is unavailable.
  """
  @spec lookup(String.t()) :: {:ok, term()} | :miss
  def lookup(filter_str) do
    case :ets.lookup(@table, filter_str) do
      [{^filter_str, ast}] -> {:ok, ast}
      [] -> :miss
    end
  rescue
    ArgumentError -> :miss
  end

  @doc "Memoize `ast` for `filter_str`, emptying the cache when it is full."
  @spec put(String.t(), term()) :: :ok
  def put(filter_str, ast) do
    if size() >= @limit, do: clear()

    :ets.insert(@table, {filter_str, ast})
    :ok
  rescue
    ArgumentError -> :ok
  end

  @doc "Empty the cache."
  @spec clear() :: :ok
  def clear do
    :ets.delete_all_objects(@table)
    :ok
  rescue
    ArgumentError -> :ok
  end

  @doc "Whether `filter_str` is currently memoized."
  @spec member?(String.t()) :: boolean()
  def member?(filter_str) do
    :ets.member(@table, filter_str)
  rescue
    ArgumentError -> false
  end

  @doc "Number of memoized filters (0 when the table is unavailable)."
  @spec size() :: non_neg_integer()
  def size do
    case :ets.whereis(@table) do
      :undefined -> 0
      _table -> :ets.info(@table, :size)
    end
  end

  @impl true
  def init(_opts) do
    :ets.new(@table, [
      :named_table,
      :public,
      :set,
      read_concurrency: true,
      write_concurrency: true
    ])

    {:ok, %{}}
  end
end
