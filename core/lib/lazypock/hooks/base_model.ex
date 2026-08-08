defmodule Lazypock.Hooks.BaseModel do
  @moduledoc """
  PocketBase base Model hooks — fired for all PocketBase structs implementing
  the Model DB interface (Record, Collection, Log, etc.).

  Full parity with the PocketBase docs:

    * `onModelValidate` — model validation
    * Create: `onModelCreate` → `onModelValidate` → `onModelCreateExecute`
      → `onModelAfterCreateSuccess` / `onModelAfterCreateError`
    * Update: `onModelUpdate` → `onModelValidate` → `onModelUpdateExecute`
      → `onModelAfterUpdateSuccess` / `onModelAfterUpdateError`
    * Delete: `onModelDelete` → internal checks → `onModelDeleteExecute`
      → `onModelAfterDeleteSuccess` / `onModelAfterDeleteError`

  ## Event fields (PocketBase parity)

    * `e.app` — the app
    * `e.model` — the model being created/updated/deleted/validated
    * `e.error` — the error (only for `*Error` hooks)
    * `e.collection_name` — optional name hint for filtering

  Handlers use the `function(e)` chain convention.
  """

  alias Lazypock.Hooks.Registry
  alias Lazypock.Hooks.Event

  # ── Registration helpers ────────────────────────────────

  @doc "Registers an `on_model_validate` handler."
  def on_model_validate(fun, opts \\ []), do: register_model(:on_model_validate, fun, opts)

  @doc "Registers an `on_model_create` handler."
  def on_model_create(fun, opts \\ []), do: register_model(:on_model_create, fun, opts)

  @doc "Registers an `on_model_create_execute` handler."
  def on_model_create_execute(fun, opts \\ []),
    do: register_model(:on_model_create_execute, fun, opts)

  @doc "Registers an `on_model_after_create_success` handler."
  def on_model_after_create_success(fun, opts \\ []),
    do: register_model(:on_model_after_create_success, fun, opts)

  @doc "Registers an `on_model_after_create_error` handler."
  def on_model_after_create_error(fun, opts \\ []),
    do: register_model(:on_model_after_create_error, fun, opts)

  @doc "Registers an `on_model_update` handler."
  def on_model_update(fun, opts \\ []), do: register_model(:on_model_update, fun, opts)

  @doc "Registers an `on_model_update_execute` handler."
  def on_model_update_execute(fun, opts \\ []),
    do: register_model(:on_model_update_execute, fun, opts)

  @doc "Registers an `on_model_after_update_success` handler."
  def on_model_after_update_success(fun, opts \\ []),
    do: register_model(:on_model_after_update_success, fun, opts)

  @doc "Registers an `on_model_after_update_error` handler."
  def on_model_after_update_error(fun, opts \\ []),
    do: register_model(:on_model_after_update_error, fun, opts)

  @doc "Registers an `on_model_delete` handler."
  def on_model_delete(fun, opts \\ []), do: register_model(:on_model_delete, fun, opts)

  @doc "Registers an `on_model_delete_execute` handler."
  def on_model_delete_execute(fun, opts \\ []),
    do: register_model(:on_model_delete_execute, fun, opts)

  @doc "Registers an `on_model_after_delete_success` handler."
  def on_model_after_delete_success(fun, opts \\ []),
    do: register_model(:on_model_after_delete_success, fun, opts)

  @doc "Registers an `on_model_after_delete_error` handler."
  def on_model_after_delete_error(fun, opts \\ []),
    do: register_model(:on_model_after_delete_error, fun, opts)

  defp register_model(event, fun, opts) when is_function(fun, 1) do
    Registry.register(event, {:fun, fun}, opts)
    :ok
  end

  defp register_model(_event, _fun, _opts), do: {:error, :invalid_handler}

  # ── Trigger points (called by the framework) ────────────

  @doc "Fires `on_model_validate`. Returns `{:ok, event, after_funs}` or `{:error, reason}`."
  def trigger_validate(model, collection_name \\ nil) do
    trigger(:on_model_validate, %{model: model}, collection_name)
  end

  @doc "Fires `on_model_create`."
  def trigger_create(model, collection_name \\ nil) do
    trigger(:on_model_create, %{model: model}, collection_name)
  end

  @doc "Fires `on_model_create_execute`."
  def trigger_create_execute(model, collection_name \\ nil) do
    trigger(:on_model_create_execute, %{model: model}, collection_name)
  end

  @doc "Fires `on_model_after_create_success`."
  def trigger_after_create_success(model, collection_name \\ nil) do
    trigger(:on_model_after_create_success, %{model: model}, collection_name)
  end

  @doc "Fires `on_model_after_create_error`."
  def trigger_after_create_error(model, error, collection_name \\ nil) do
    trigger(:on_model_after_create_error, %{model: model, error: error}, collection_name)
  end

  @doc "Fires `on_model_update`."
  def trigger_update(model, collection_name \\ nil) do
    trigger(:on_model_update, %{model: model}, collection_name)
  end

  @doc "Fires `on_model_update_execute`."
  def trigger_update_execute(model, collection_name \\ nil) do
    trigger(:on_model_update_execute, %{model: model}, collection_name)
  end

  @doc "Fires `on_model_after_update_success`."
  def trigger_after_update_success(model, collection_name \\ nil) do
    trigger(:on_model_after_update_success, %{model: model}, collection_name)
  end

  @doc "Fires `on_model_after_update_error`."
  def trigger_after_update_error(model, error, collection_name \\ nil) do
    trigger(:on_model_after_update_error, %{model: model, error: error}, collection_name)
  end

  @doc "Fires `on_model_delete`."
  def trigger_delete(model, collection_name \\ nil) do
    trigger(:on_model_delete, %{model: model}, collection_name)
  end

  @doc "Fires `on_model_delete_execute`."
  def trigger_delete_execute(model, collection_name \\ nil) do
    trigger(:on_model_delete_execute, %{model: model}, collection_name)
  end

  @doc "Fires `on_model_after_delete_success`."
  def trigger_after_delete_success(model, collection_name \\ nil) do
    trigger(:on_model_after_delete_success, %{model: model}, collection_name)
  end

  @doc "Fires `on_model_after_delete_error`."
  def trigger_after_delete_error(model, error, collection_name \\ nil) do
    trigger(:on_model_after_delete_error, %{model: model, error: error}, collection_name)
  end

  defp trigger(event, data, collection_name) do
    case Registry.dispatch(event, data, collection_name) do
      {:ok, %Event{} = ev} -> {:ok, ev, []}
      other -> other
    end
  end
end
