defmodule Lazypock.Hooks.Record do
  @moduledoc """
  PocketBase Record model hooks — the lower-level Record persistence hooks
  that fire from anywhere (custom code, cron, `$app.save()`, etc.) and have
  NO access to the request context.

  Full parity with the PocketBase docs:

    * `onRecordEnrich` — fired when a record is enriched for API responses
    * `onRecordValidate` — record validation
    * Create: `onRecordCreate` → `onRecordValidate` → `onRecordCreateExecute`
      → `onRecordAfterCreateSuccess` / `onRecordAfterCreateError`
    * Update: `onRecordUpdate` → `onRecordValidate` → `onRecordUpdateExecute`
      → `onRecordAfterUpdateSuccess` / `onRecordAfterUpdateError`
    * Delete: `onRecordDelete` → internal checks → `onRecordDeleteExecute`
      → `onRecordAfterDeleteSuccess` / `onRecordAfterDeleteError`

  ## Event fields (PocketBase parity)

    * `e.app` — the app
    * `e.record` — the record being created/updated/deleted/validated
    * `e.error` — the error (only for `*Error` hooks)
    * `e.collection` — the collection
    * `e.collection_name` — the collection name (for filtering)

  Handlers use the `function(e)` chain convention: return `{:ok, e}` to
  proceed, `{:error, reason}` to abort, or `Event.next(e, post_fun)` to run
  post-action work ("operations AFTER `e.next()`").
  """

  alias Lazypock.Hooks.Event
  alias Lazypock.Hooks.Registry

  # ── Registration helpers ────────────────────────────────

  @doc "Registers an `on_record_enrich` handler (fires for `collections` or all)."
  def on_record_enrich(fun, opts \\ []) do
    register_record(:on_record_enrich, fun, opts)
  end

  @doc "Registers an `on_record_validate` handler."
  def on_record_validate(fun, opts \\ []), do: register_record(:on_record_validate, fun, opts)

  @doc "Registers an `on_record_create` handler."
  def on_record_create(fun, opts \\ []), do: register_record(:on_record_create, fun, opts)

  @doc "Registers an `on_record_create_execute` handler."
  def on_record_create_execute(fun, opts \\ []),
    do: register_record(:on_record_create_execute, fun, opts)

  @doc "Registers an `on_record_after_create_success` handler."
  def on_record_after_create_success(fun, opts \\ []),
    do: register_record(:on_record_after_create_success, fun, opts)

  @doc "Registers an `on_record_after_create_error` handler."
  def on_record_after_create_error(fun, opts \\ []),
    do: register_record(:on_record_after_create_error, fun, opts)

  @doc "Registers an `on_record_update` handler."
  def on_record_update(fun, opts \\ []), do: register_record(:on_record_update, fun, opts)

  @doc "Registers an `on_record_update_execute` handler."
  def on_record_update_execute(fun, opts \\ []),
    do: register_record(:on_record_update_execute, fun, opts)

  @doc "Registers an `on_record_after_update_success` handler."
  def on_record_after_update_success(fun, opts \\ []),
    do: register_record(:on_record_after_update_success, fun, opts)

  @doc "Registers an `on_record_after_update_error` handler."
  def on_record_after_update_error(fun, opts \\ []),
    do: register_record(:on_record_after_update_error, fun, opts)

  @doc "Registers an `on_record_delete` handler."
  def on_record_delete(fun, opts \\ []), do: register_record(:on_record_delete, fun, opts)

  @doc "Registers an `on_record_delete_execute` handler."
  def on_record_delete_execute(fun, opts \\ []),
    do: register_record(:on_record_delete_execute, fun, opts)

  @doc "Registers an `on_record_after_delete_success` handler."
  def on_record_after_delete_success(fun, opts \\ []),
    do: register_record(:on_record_after_delete_success, fun, opts)

  @doc "Registers an `on_record_after_delete_error` handler."
  def on_record_after_delete_error(fun, opts \\ []),
    do: register_record(:on_record_after_delete_error, fun, opts)

  defp register_record(event, fun, opts) when is_function(fun, 1) do
    Registry.register(event, {:fun, fun}, opts)
    :ok
  end

  defp register_record(_event, _fun, _opts), do: {:error, :invalid_handler}

  # ── Trigger points (called by the framework) ────────────

  @doc """
  Fires `on_record_enrich` (collect-only: each handler receives the record
  and returns a possibly-modified event; the last successful event's `record`
  is used). Returns `{:ok, record}`.
  """
  def trigger_enrich(record, collection_name, request_info \\ nil) do
    case Registry.dispatch(
           :on_record_enrich,
           %{record: record, request_info: request_info},
           collection_name, collect_only: true) do
      {:ok, %Event{} = ev} -> {:ok, Event.get(ev, :record) || record}
      {:error, _} -> {:ok, record}
    end
  end

  @doc """
  Fires the record validate chain. Returns `{:ok, event}` or `{:error, reason}`.
  """
  def trigger_validate(record, collection, collection_name) do
    trigger(:on_record_validate, record, collection, collection_name)
  end

  @doc """
  Fires `on_record_create` (before validation + INSERT).
  Returns `{:ok, event, after_funs}` or `{:error, reason}`.
  """
  def trigger_create(record, collection, collection_name) do
    trigger(:on_record_create, record, collection, collection_name)
  end

  @doc "Fires `on_record_create_execute` (right before INSERT)."
  def trigger_create_execute(record, collection, collection_name) do
    trigger(:on_record_create_execute, record, collection, collection_name)
  end

  @doc "Fires `on_record_after_create_success` (after commit)."
  def trigger_after_create_success(record, collection, collection_name) do
    trigger(:on_record_after_create_success, record, collection, collection_name)
  end

  @doc "Fires `on_record_after_create_error`."
  def trigger_after_create_error(record, error, collection, collection_name) do
    trigger_with_error(:on_record_after_create_error, record, error, collection, collection_name)
  end

  @doc "Fires `on_record_update` (before validation + UPDATE)."
  def trigger_update(record, collection, collection_name) do
    trigger(:on_record_update, record, collection, collection_name)
  end

  @doc "Fires `on_record_update_execute` (right before UPDATE)."
  def trigger_update_execute(record, collection, collection_name) do
    trigger(:on_record_update_execute, record, collection, collection_name)
  end

  @doc "Fires `on_record_after_update_success` (after commit)."
  def trigger_after_update_success(record, collection, collection_name) do
    trigger(:on_record_after_update_success, record, collection, collection_name)
  end

  @doc "Fires `on_record_after_update_error`."
  def trigger_after_update_error(record, error, collection, collection_name) do
    trigger_with_error(:on_record_after_update_error, record, error, collection, collection_name)
  end

  @doc "Fires `on_record_delete` (before internal delete checks)."
  def trigger_delete(record, collection, collection_name) do
    trigger(:on_record_delete, record, collection, collection_name)
  end

  @doc "Fires `on_record_delete_execute` (right before DELETE)."
  def trigger_delete_execute(record, collection, collection_name) do
    trigger(:on_record_delete_execute, record, collection, collection_name)
  end

  @doc "Fires `on_record_after_delete_success` (after commit)."
  def trigger_after_delete_success(record, collection, collection_name) do
    trigger(:on_record_after_delete_success, record, collection, collection_name)
  end

  @doc "Fires `on_record_after_delete_error`."
  def trigger_after_delete_error(record, error, collection, collection_name) do
    trigger_with_error(:on_record_after_delete_error, record, error, collection, collection_name)
  end

  # ── Shared trigger machinery ────────────────────────────

  defp trigger(event, record, collection, collection_name) do
    data = %{record: record, collection: collection}

    # Normalize to the stable {:ok, event, after_funs} contract
    case Registry.dispatch(event, data, collection_name) do
      {:ok, %Event{} = ev} -> {:ok, ev, []}
      other -> other
    end
  end

  defp trigger_with_error(event, record, error, collection, collection_name) do
    data = %{record: record, error: error, collection: collection}

    case Registry.dispatch(event, data, collection_name) do
      {:ok, %Event{} = ev} -> {:ok, ev, []}
      other -> other
    end
  end
end
