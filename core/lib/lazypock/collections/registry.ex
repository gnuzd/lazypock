defmodule Lazypock.Collections.Registry do
  @moduledoc """
  In-memory cache of all collections and their field definitions.

  Backed by an ETS table for fast reads (O(1) lookups). The registry
  is populated from the database at startup and kept in sync across
  cluster nodes via Phoenix PubSub broadcasts from the DDL engine.

  ## Usage

      # Look up a collection by name
      Lazypock.Collections.Registry.get("posts")
      # => {:ok, %Lazypock.Collections.Collection{...}}

      # List all collections
      Lazypock.Collections.Registry.list()
      # => [%Collection{...}, ...]
  """

  use GenServer

  alias Lazypock.Repo
  alias Lazypock.Collections.Collection

  @table_name :lazypock_collections

  # ── Public API ────────────────────────────────────────────

  @doc """
  Starts the registry GenServer.
  Called by the application supervision tree.
  """
  @spec start_link(keyword()) :: GenServer.on_start()
  def start_link(opts \\ []) do
    GenServer.start_link(__MODULE__, %{}, opts ++ [name: __MODULE__])
  end

  @doc """
  Looks up a collection by its name.

  Returns `{:ok, collection}` if found, `{:error, :not_found}` otherwise.
  """
  @spec get(String.t()) :: {:ok, Collection.t()} | {:error, :not_found}
  def get(name) when is_binary(name) do
    case :ets.lookup(@table_name, name) do
      [{^name, collection}] -> {:ok, collection}
      [] -> {:error, :not_found}
    end
  end

  @doc """
  Returns all registered collections as a list.
  """
  @spec list() :: [Collection.t()]
  def list do
    :ets.tab2list(@table_name)
    |> Enum.map(&elem(&1, 1))
  end

  @doc """
  Forces a reload of the entire registry from the database.
  Useful in development or after restoring from backup.
  """
  @spec reload!() :: :ok
  def reload! do
    GenServer.call(__MODULE__, :reload)
  end

  @doc """
  Returns the number of collections in the cache.
  """
  @spec count() :: non_neg_integer()
  def count do
    :ets.info(@table_name, :size)
  end

  # ── GenServer callbacks ──────────────────────────────────

  @impl true
  def init(_opts) do
    table =
      :ets.new(@table_name, [
        :named_table,
        :protected,
        :set,
        read_concurrency: true
      ])

    load_all_into_cache()

    # Subscribe to schema change broadcasts
    Phoenix.PubSub.subscribe(Lazypock.PubSub, "schema")

    {:ok, %{table: table}}
  end

  @impl true
  def handle_call(:reload, _from, state) do
    load_all_into_cache()
    {:reply, :ok, state}
  end

  @impl true
  def handle_info({:collection_created, %Lazypock.Collections.Collection{} = collection}, state) do
    # Reload the specific collection with its fields preloaded
    collection = Repo.preload(collection, :fields)
    :ets.insert(@table_name, {collection.name, collection})
    {:noreply, state}
  end

  # Defense in depth: never crash on a malformed broadcast (e.g. a buggy
  # caller broadcasting {:error, reason} as the collection payload).
  def handle_info({:collection_created, _malformed}, state) do
    load_all_into_cache()
    {:noreply, state}
  end

  @impl true
  def handle_info({:field_added, _collection_name, _field_def}, state) do
    # Easiest: reload all. For a large number of collections, optimize later.
    load_all_into_cache()
    {:noreply, state}
  end

  @impl true
  def handle_info({:field_removed, _collection_name, _field_name}, state) do
    load_all_into_cache()
    {:noreply, state}
  end

  @impl true
  def handle_info({:collection_updated, %Lazypock.Collections.Collection{} = collection}, state) do
    # Reload the specific collection with its fields preloaded
    collection = Repo.preload(collection, :fields)
    :ets.insert(@table_name, {collection.name, collection})
    {:noreply, state}
  end

  # Defense in depth: never crash on a malformed broadcast payload.
  def handle_info({:collection_updated, _malformed}, state) do
    load_all_into_cache()
    {:noreply, state}
  end

  @impl true
  def handle_info({:collection_deleted, name}, state) do
    :ets.delete(@table_name, name)
    {:noreply, state}
  end

  # ── Private ──────────────────────────────────────────────

  @impl true
  def handle_info(_msg, state) do
    # Ignore unknown messages (e.g. schema broadcasts we don't handle)
    {:noreply, state}
  end

  defp load_all_into_cache do
    # Clear existing cache
    :ets.delete_all_objects(@table_name)

    # Load all collections with fields from DB
    Repo.all(Collection)
    |> Repo.preload(:fields)
    |> Enum.each(fn collection ->
      :ets.insert(@table_name, {collection.name, collection})
    end)
  end
end
