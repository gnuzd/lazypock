defmodule Lazypock.Hooks.Collection do
  @moduledoc """
  PocketBase Collection model hooks — the lower-level Collection persistence
  hooks that fire from anywhere and have NO access to the request context.

  Full parity with the PocketBase docs:

    * `onCollectionValidate` — collection validation
    * Create: `onCollectionCreate` → `onCollectionValidate`
      → `onCollectionCreateExecute` → `onCollectionAfterCreateSuccess` /
      `onCollectionAfterCreateError`
    * Update: `onCollectionUpdate` → `onCollectionValidate`
      → `onCollectionUpdateExecute` → `onCollectionAfterUpdateSuccess` /
      `onCollectionAfterUpdateError`
    * Delete: `onCollectionDelete` → internal checks → `onCollectionDeleteExecute`
      → `onCollectionAfterDeleteSuccess` / `onCollectionAfterDeleteError`

  ## Event fields (PocketBase parity)

    * `e.app` — the app
    * `e.collection` — the collection being created/updated/deleted/validated
    * `e.error` — the error (only for `*Error` hooks)
    * `e.collection_name` — the collection name (for filtering)

  Handlers use the `function(e)` chain convention.
  """

  alias Lazypock.Hooks.Registry
  alias Lazypock.Hooks.Event

  # ── Registration helpers ────────────────────────────────

  @doc "Registers an `on_collection_validate` handler."
  def on_collection_validate(fun, opts \\ []),
    do: register_collection(:on_collection_validate, fun, opts)

  @doc "Registers an `on_collection_create` handler."
  def on_collection_create(fun, opts \\ []),
    do: register_collection(:on_collection_create, fun, opts)

  @doc "Registers an `on_collection_create_execute` handler."
  def on_collection_create_execute(fun, opts \\ []),
    do: register_collection(:on_collection_create_execute, fun, opts)

  @doc "Registers an `on_collection_after_create_success` handler."
  def on_collection_after_create_success(fun, opts \\ []),
    do: register_collection(:on_collection_after_create_success, fun, opts)

  @doc "Registers an `on_collection_after_create_error` handler."
  def on_collection_after_create_error(fun, opts \\ []),
    do: register_collection(:on_collection_after_create_error, fun, opts)

  @doc "Registers an `on_collection_update` handler."
  def on_collection_update(fun, opts \\ []),
    do: register_collection(:on_collection_update, fun, opts)

  @doc "Registers an `on_collection_update_execute` handler."
  def on_collection_update_execute(fun, opts \\ []),
    do: register_collection(:on_collection_update_execute, fun, opts)

  @doc "Registers an `on_collection_after_update_success` handler."
  def on_collection_after_update_success(fun, opts \\ []),
    do: register_collection(:on_collection_after_update_success, fun, opts)

  @doc "Registers an `on_collection_after_update_error` handler."
  def on_collection_after_update_error(fun, opts \\ []),
    do: register_collection(:on_collection_after_update_error, fun, opts)

  @doc "Registers an `on_collection_delete` handler."
  def on_collection_delete(fun, opts \\ []),
    do: register_collection(:on_collection_delete, fun, opts)

  @doc "Registers an `on_collection_delete_execute` handler."
  def on_collection_delete_execute(fun, opts \\ []),
    do: register_collection(:on_collection_delete_execute, fun, opts)

  @doc "Registers an `on_collection_after_delete_success` handler."
  def on_collection_after_delete_success(fun, opts \\ []),
    do: register_collection(:on_collection_after_delete_success, fun, opts)

  @doc "Registers an `on_collection_after_delete_error` handler."
  def on_collection_after_delete_error(fun, opts \\ []),
    do: register_collection(:on_collection_after_delete_error, fun, opts)

  defp register_collection(event, fun, opts) when is_function(fun, 1) do
    Registry.register(event, {:fun, fun}, opts)
    :ok
  end

  defp register_collection(_event, _fun, _opts), do: {:error, :invalid_handler}

  # ── Trigger points (called by the framework) ────────────

  @doc "Fires `on_collection_validate`. Returns `{:ok, event, after_funs}` or `{:error, reason}`."
  def trigger_validate(collection) do
    trigger(:on_collection_validate, collection, %{collection: collection})
  end

  @doc "Fires `on_collection_create`."
  def trigger_create(collection) do
    trigger(:on_collection_create, collection, %{collection: collection})
  end

  @doc "Fires `on_collection_create_execute`."
  def trigger_create_execute(collection) do
    trigger(:on_collection_create_execute, collection, %{collection: collection})
  end

  @doc "Fires `on_collection_after_create_success`."
  def trigger_after_create_success(collection) do
    trigger(:on_collection_after_create_success, collection, %{collection: collection})
  end

  @doc "Fires `on_collection_after_create_error`."
  def trigger_after_create_error(collection, error) do
    trigger(:on_collection_after_create_error, collection, %{collection: collection, error: error})
  end

  @doc "Fires `on_collection_update`."
  def trigger_update(collection) do
    trigger(:on_collection_update, collection, %{collection: collection})
  end

  @doc "Fires `on_collection_update_execute`."
  def trigger_update_execute(collection) do
    trigger(:on_collection_update_execute, collection, %{collection: collection})
  end

  @doc "Fires `on_collection_after_update_success`."
  def trigger_after_update_success(collection) do
    trigger(:on_collection_after_update_success, collection, %{collection: collection})
  end

  @doc "Fires `on_collection_after_update_error`."
  def trigger_after_update_error(collection, error) do
    trigger(:on_collection_after_update_error, collection, %{collection: collection, error: error})
  end

  @doc "Fires `on_collection_delete`."
  def trigger_delete(collection) do
    trigger(:on_collection_delete, collection, %{collection: collection})
  end

  @doc "Fires `on_collection_delete_execute`."
  def trigger_delete_execute(collection) do
    trigger(:on_collection_delete_execute, collection, %{collection: collection})
  end

  @doc "Fires `on_collection_after_delete_success`."
  def trigger_after_delete_success(collection) do
    trigger(:on_collection_after_delete_success, collection, %{collection: collection})
  end

  @doc "Fires `on_collection_after_delete_error`."
  def trigger_after_delete_error(collection, error) do
    trigger(:on_collection_after_delete_error, collection, %{collection: collection, error: error})
  end

  defp trigger(event, collection, data) do
    case Registry.dispatch(event, data, collection.name) do
      {:ok, %Event{} = ev} -> {:ok, ev, []}
      other -> other
    end
  end
end
