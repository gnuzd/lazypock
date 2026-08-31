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

    # Snapshot table for view-collection realtime diffs. Owned by this
    # long-lived GenServer so its lifetime isn't tied to per-request
    # processes (a request-scoped owner would destroy the table — and the
    # view diff state — when the request process exits).
    :ets.new(:lazypock_view_rows, [
      :named_table,
      :public,
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

  # Synchronous variant of the PubSub broadcast handlers below. DDL calls
  # this under test (see DDL.safe_broadcast/2) so a schema change's cache
  # update — which hits the DB in this process — is finished before the DDL
  # call returns. Otherwise the reload can still be in flight when the test's
  # sandbox owner exits, which logs Postgrex "owner exited" disconnect noise.
  @impl true
  def handle_call({:apply_event, message}, _from, state) do
    handle_broadcast(message)
    {:reply, :ok, state}
  end

  @impl true
  def handle_info(message, state) do
    handle_broadcast(message)
    {:noreply, state}
  end

  defp handle_broadcast({:collection_created, %Lazypock.Collections.Collection{} = collection}) do
    # Reload the specific collection with its fields preloaded
    collection = Repo.preload(collection, :fields)
    :ets.insert(@table_name, {collection.name, collection})
    :ok
  end

  # Defense in depth: never crash on a malformed broadcast (e.g. a buggy
  # caller broadcasting {:error, reason} as the collection payload).
  defp handle_broadcast({:collection_created, _malformed}) do
    load_all_into_cache()
    :ok
  end

  defp handle_broadcast({:field_added, _collection_name, _field_def}) do
    # Easiest: reload all. For a large number of collections, optimize later.
    load_all_into_cache()
    :ok
  end

  defp handle_broadcast({:field_removed, _collection_name, _field_name}) do
    load_all_into_cache()
    :ok
  end

  defp handle_broadcast({:collection_updated, %Lazypock.Collections.Collection{} = collection}) do
    # Reload the specific collection with its fields preloaded
    collection = Repo.preload(collection, :fields)
    :ets.insert(@table_name, {collection.name, collection})
    :ok
  end

  # Defense in depth: never crash on a malformed broadcast payload.
  defp handle_broadcast({:collection_updated, _malformed}) do
    load_all_into_cache()
    :ok
  end

  defp handle_broadcast({:collection_deleted, name}) do
    :ets.delete(@table_name, name)
    :ok
  end

  defp handle_broadcast(_msg) do
    # Ignore unknown messages (e.g. schema broadcasts we don't handle)
    :ok
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
