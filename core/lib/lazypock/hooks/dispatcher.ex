defmodule Lazypock.Hooks.Dispatcher do
  @moduledoc """
  Orchestrates hook execution across all layers.

  ## Pipeline

  For each CRUD action:
    1. Layer 1 Declarative hooks (pre) — modify attrs or reject
    2. Layer 2 File-based hooks (pre) — modify attrs or reject
    3. Database operation
    4. Layer 1 Declarative hooks (after) — fire-and-forget
    5. Layer 2 File-based hooks (after) — fire-and-forget
  """

  alias Lazypock.Hooks.Registry

  @doc """
  Runs pre-create hooks. Returns `{:ok, modified_attrs}` or `{:error, reason}`.
  """
  def dispatch_create(attrs, context \\ %{}) do
    collection_name = context.collection_name

    pipeline = [
      &run_declarative_hooks(:onCreate, collection_name, &1, &2),
      &run_file_hooks(:on_create, collection_name, &1, &2)
    ]

    run_pipeline(pipeline, attrs, context)
  end

  @doc """
  Runs after-create hooks (fire-and-forget).
  """
  def dispatch_after_create(record, context \\ %{}) do
    collection_name = context.collection_name

    Task.start(fn ->
      run_declarative_hooks(:afterCreate, collection_name, record, context)
      run_file_hooks(:after_create, collection_name, record, context)
    end)

    :ok
  end

  @doc """
  Runs pre-update hooks. Returns `{:ok, modified_attrs}` or `{:error, reason}`.
  """
  def dispatch_update(old_record, new_attrs, context \\ %{}) do
    collection_name = context.collection_name

    pipeline = [
      &run_file_hooks(:on_update, collection_name, &1, &2)
    ]

    run_pipeline(pipeline, {old_record, new_attrs}, context)
  end

  @doc """
  Runs pre-delete hooks. Returns `:ok` or `{:error, reason}`.
  """
  @spec dispatch_delete(map(), term()) :: :ok | {:error, term()}
  def dispatch_delete(record, context \\ %{}) do
    collection_name = context.collection_name

    # For delete, hooks just return :ok or {:error, reason}
    modules = Registry.get(collection_name)

    Enum.reduce_while(modules, :ok, fn module, _acc ->
      case module.on_delete(record, context) do
        :ok -> {:cont, :ok}
        {:error, reason} -> {:halt, {:error, reason}}
      end
    end)
  end

  # ── Pipeline runner ─────────────────────────────────

  defp run_pipeline(steps, initial_data, context) do
    Enum.reduce_while(steps, {:ok, initial_data}, fn step, {:ok, data} ->
      case step.(data, context) do
        {:ok, modified} -> {:cont, {:ok, modified}}
        {:error, reason} -> {:halt, {:error, reason}}
      end
    end)
  end

  # ── Layer 1: Declarative hooks ──────────────────────

  defp run_declarative_hooks(_event, _collection_name, data, _context) do
    # Declarative hooks stored in _collections.hooks JSONB
    # For now, return data unchanged — will be implemented fully
    # when the Admin UI (Phase 8) provides a builder for these
    {:ok, data}
  end

  # ── Layer 2: File-based hooks ───────────────────────

  defp run_file_hooks(:on_create, collection_name, attrs, context) do
    modules = Registry.get(collection_name)

    Enum.reduce_while(modules, {:ok, attrs}, fn module, {:ok, current_attrs} ->
      case module.on_create(current_attrs, context) do
        {:ok, modified} -> {:cont, {:ok, modified}}
        {:error, reason} -> {:halt, {:error, reason}}
      end
    end)
  end

  defp run_file_hooks(:after_create, collection_name, record, context) do
    Registry.get(collection_name)
    |> Enum.each(fn module ->
      module.after_create(record, context)
    end)

    {:ok, record}
  end

  defp run_file_hooks(:on_update, collection_name, {old_record, new_attrs}, context) do
    modules = Registry.get(collection_name)

    Enum.reduce_while(modules, {:ok, new_attrs}, fn module, {:ok, current_attrs} ->
      case module.on_update(old_record, current_attrs, context) do
        {:ok, modified} -> {:cont, {:ok, modified}}
        {:error, reason} -> {:halt, {:error, reason}}
      end
    end)
  end
end
