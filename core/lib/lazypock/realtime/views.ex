defmodule Lazypock.Realtime.Views do
  @moduledoc """
  Realtime support for view collections (a superset of PocketBase, which
  doesn't emit realtime events for views).

  After any successful record mutation on a base/auth collection, the rows of
  every registered view collection are re-selected, diffed against a cached
  snapshot, and `create` / `update` / `delete` events are broadcast to
  `collection:<view>` subscribers — so clients subscribed to a view see it
  change live as its source data changes.

  The snapshot is kept in an ETS table keyed by view name. The first
  observation after boot (or after a view is re-saved) seeds the snapshot
  without broadcasting, so subscribers don't get spurious `create` events for
  pre-existing rows.

  Multi-node note: the diff+broadcast runs on the node that handled the
  mutation (only one node per request), so events are not duplicated; other
  nodes' snapshots refresh lazily on their next local mutation.
  """

  alias Lazypock.Collections.Registry
  alias Lazypock.Realtime.Broadcaster
  alias Lazypock.Repo
  alias Lazypock.Schema.TypeMapper
  alias Lazypock.Schemas.FieldNames

  @table :lazypock_view_rows

  @doc """
  Ensures the ETS snapshot table exists. The table is normally created by
  `Lazypock.Collections.Registry` (a long-lived GenServer) so its lifetime
  isn't tied to any per-request process; this fallback only covers the edge
  case where the table is missing (e.g. during hot code reload).
  """
  @spec init() :: :ok
  def init do
    if :ets.whereis(@table) == :undefined do
      # Owner = the calling process. Prefer the Registry-created table; this
      # fallback is ephemeral (dies with the request process) but never
      # resurrects a snapshot that would double-broadcast.
      :ets.new(@table, [:named_table, :public, :set, read_concurrency: true])
    end

    :ok
  end

  @doc """
  Drops the cached snapshot for a view so the next mutation re-seeds it
  instead of diffing against stale rows. Called by the DDL engine after a
  view is created, rebuilt, renamed or dropped.
  """
  @spec reset_view(String.t()) :: :ok
  def reset_view(name) do
    if :ets.whereis(@table) != :undefined do
      :ets.delete(@table, name)
    end

    :ok
  end

  @doc """
  Called after a successful record mutation (create/update/delete) on a
  base/auth collection. Re-selects every view collection, diffs against the
  cached snapshot, broadcasts the changes, and stores the new snapshot.

  No-op when no view collections are registered or when the table isn't
  initialized (the table is created lazily on the first call).
  """
  @spec after_mutation(String.t()) :: :ok
  def after_mutation(_source_collection) do
    init()

    views = Enum.filter(Registry.list(), &(&1.type == "view"))

    if views != [] do
      Enum.each(views, &sync_view/1)
    end

    :ok
  end

  # ── Private ────────────────────────────────────────

  defp sync_view(collection) do
    name = collection.name
    rows = fetch_rows(collection)
    new_snapshot = Map.new(rows, &{&1["id"], &1})
    new_ids = Map.keys(new_snapshot)

    case :ets.lookup(@table, name) do
      [{^name, old_snapshot}] ->
        old_ids = Map.keys(old_snapshot)

        for id <- new_ids, not Map.has_key?(old_snapshot, id) do
          Broadcaster.broadcast_create(name, new_snapshot[id])
        end

        for id <- old_ids, not Map.has_key?(new_snapshot, id) do
          Broadcaster.broadcast_delete(name, id)
        end

        for id <- new_ids,
            Map.has_key?(old_snapshot, id),
            old_snapshot[id] != new_snapshot[id] do
          Broadcaster.broadcast_update(name, new_snapshot[id])
        end

        :ets.insert(@table, {name, new_snapshot})

      [] ->
        # First observation — seed without broadcasting.
        :ets.insert(@table, {name, new_snapshot})
    end
  end

  defp fetch_rows(collection) do
    sql = "SELECT * FROM #{TypeMapper.quote_ident(collection.name)}"

    case Ecto.Adapters.SQL.query(Repo, sql, []) do
      {:ok, %{columns: cols, rows: rows}} ->
        rows
        |> Enum.map(fn row -> cols |> Enum.zip(row) |> Map.new() end)
        |> Enum.map(&FieldNames.row_to_api(&1, collection))

      {:error, %Postgrex.Error{} = err} ->
        # A view whose query became invalid (e.g. a source column was dropped)
        # shouldn't crash mutation pipelines — log and treat as empty.
        require Logger

        Logger.error(
          "Failed to read view '#{collection.name}' for realtime diff: #{Exception.message(err)}"
        )

        []
    end
  end
end
