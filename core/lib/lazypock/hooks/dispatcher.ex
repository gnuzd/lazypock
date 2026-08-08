defmodule Lazypock.Hooks.Dispatcher do
  @moduledoc """
  Orchestrates hook execution for CRUD operations — the entry point the
  controllers call. Delegates to the PocketBase-parity hook modules:

    * `Lazypock.Hooks.Record` — record model hooks
    * `Lazypock.Hooks.Collection` — collection model hooks
    * `Lazypock.Hooks.BaseModel` — base model hooks
    * `Lazypock.Hooks.Request` — request hooks (API-only)
    * `Lazypock.Hooks.Mailer` — mailer hooks
    * `Lazypock.Hooks.Realtime` — realtime hooks
    * `Lazypock.Hooks.App` — app-level hooks

  Each hook follows the PocketBase event-chain convention: handlers receive
  an event `e` and call `e.next()` (return `{:ok, e}`) to proceed, or return
  `{:error, reason}` to abort. Handlers can also schedule "after" work via
  `Event.next(e, post_fun)` which runs after the DB action (PocketBase's
  documented "operations AFTER `e.next()`").
  """

  alias Lazypock.Hooks.{Record, Collection, Registry, Event}
  alias Lazypock.Hooks.Mailer, as: MailerHooks

  # ── Record CRUD pipeline ────────────────────────────────

  @doc """
  Runs the full record-create hook pipeline:

    1. `on_record_create` (before validation + INSERT)
    2. `on_record_validate` (skipped with `saveNoValidate` equivalent)
    3. `on_record_create_execute` (right before INSERT)

  Returns `{:ok, record, after_funs}` or `{:error, reason}`.
  """
  def dispatch_create(attrs, context) do
    collection = context.collection
    collection_name = collection.name || context.collection_name

    record = attrs

    with {:ok, event, after1} <- Record.trigger_create(record, collection, collection_name),
         {:ok, event2, after2} <-
           Record.trigger_validate(record_of(event), collection, collection_name),
         {:ok, event3, after3} <-
           Record.trigger_create_execute(record_of(event2), collection, collection_name) do
      {:ok, record_of(event3), after1 ++ after2 ++ after3}
    else
      {:error, reason} -> {:error, reason}
    end
  end

  @doc "Runs `on_record_after_create_success` (post-commit)."
  def dispatch_after_create(record, context) do
    collection = context.collection
    collection_name = collection.name || context.collection_name
    Record.trigger_after_create_success(record, collection, collection_name)
  end

  @doc "Runs `on_record_after_create_error`."
  def dispatch_after_create_error(record, error, context) do
    collection = context.collection
    collection_name = collection.name || context.collection_name
    Record.trigger_after_create_error(record, error, collection, collection_name)
  end

  @doc """
  Runs the record-update hook pipeline:

    1. `on_record_update` (before validation + UPDATE)
    2. `on_record_validate`
    3. `on_record_update_execute` (right before UPDATE)

  Returns `{:ok, record, after_funs}` or `{:error, reason}`.
  """
  def dispatch_update(old_record, new_attrs, context) do
    collection = context.collection
    collection_name = collection.name || context.collection_name

    # on_record_update carries the merged record (old + new attrs).
    merged = Map.merge(old_record, new_attrs)

    with {:ok, event, after1} <- Record.trigger_update(merged, collection, collection_name),
         {:ok, event2, after2} <-
           Record.trigger_validate(record_of(event), collection, collection_name),
         {:ok, event3, after3} <-
           Record.trigger_update_execute(record_of(event2), collection, collection_name) do
      {:ok, record_of(event3), after1 ++ after2 ++ after3}
    else
      {:error, reason} -> {:error, reason}
    end
  end

  @doc "Runs `on_record_after_update_success` (post-commit)."
  def dispatch_after_update(record, context) do
    collection = context.collection
    collection_name = collection.name || context.collection_name
    Record.trigger_after_update_success(record, collection, collection_name)
  end

  @doc "Runs `on_record_after_update_error`."
  def dispatch_after_update_error(record, error, context) do
    collection = context.collection
    collection_name = collection.name || context.collection_name
    Record.trigger_after_update_error(record, error, collection, collection_name)
  end

  @doc """
  Runs the record-delete hook pipeline:

    1. `on_record_delete` (before internal checks)
    2. `on_record_delete_execute` (right before DELETE)

  Returns `:ok` or `{:error, reason}`.
  """
  def dispatch_delete(record, context) do
    collection = context.collection
    collection_name = collection.name || context.collection_name

    with {:ok, _event, _after} <- Record.trigger_delete(record, collection, collection_name),
         {:ok, _event2, _after2} <-
           Record.trigger_delete_execute(record, collection, collection_name) do
      :ok
    else
      {:error, reason} -> {:error, reason}
    end
  end

  @doc "Runs `on_record_after_delete_success` (post-commit)."
  def dispatch_after_delete(record, context) do
    collection = context.collection
    collection_name = collection.name || context.collection_name
    Record.trigger_after_delete_success(record, collection, collection_name)
  end

  @doc "Runs `on_record_after_delete_error`."
  def dispatch_after_delete_error(record, error, context) do
    collection = context.collection
    collection_name = collection.name || context.collection_name
    Record.trigger_after_delete_error(record, error, collection, collection_name)
  end

  @doc "Runs `on_record_enrich` on a record (for API responses)."
  def dispatch_enrich(record, collection_name, request_info \\ nil) do
    Record.trigger_enrich(record, collection_name, request_info)
  end

  # ── Collection CRUD pipeline ────────────────────────────

  @doc "Runs the collection-create hook pipeline."
  def dispatch_collection_create(collection) do
    with {:ok, _e, _a} <- Collection.trigger_create(collection),
         {:ok, _e2, _a2} <- Collection.trigger_validate(collection),
         {:ok, _e3, _a3} <- Collection.trigger_create_execute(collection) do
      :ok
    end
  end

  @doc "Runs `on_collection_after_create_success`."
  def dispatch_collection_after_create(collection) do
    Collection.trigger_after_create_success(collection)
  end

  @doc "Runs the collection-update hook pipeline."
  def dispatch_collection_update(collection) do
    with {:ok, _e, _a} <- Collection.trigger_update(collection),
         {:ok, _e2, _a2} <- Collection.trigger_validate(collection),
         {:ok, _e3, _a3} <- Collection.trigger_update_execute(collection) do
      :ok
    end
  end

  @doc "Runs `on_collection_after_update_success`."
  def dispatch_collection_after_update(collection) do
    Collection.trigger_after_update_success(collection)
  end

  @doc "Runs the collection-delete hook pipeline."
  def dispatch_collection_delete(collection) do
    with {:ok, _e, _a} <- Collection.trigger_delete(collection),
         {:ok, _e2, _a2} <- Collection.trigger_delete_execute(collection) do
      :ok
    end
  end

  @doc "Runs `on_collection_after_delete_success`."
  def dispatch_collection_after_delete(collection) do
    Collection.trigger_after_delete_success(collection)
  end

  @doc """
  Dispatches email interception hooks (backwards-compatible with the old
  dispatcher API used by `Lazypock.Emails`).

  Returns `{:ok, modified_email_data}`, `{:error, reason}`, or `:skip`.
  """
  def dispatch_email(template, email_data, context) do
    collection = context.collection
    collection_name = collection.name || collection.collection_name
    record = context.user || %{}

    # Build a Swoosh-style message from the email_data map (PocketBase e.message)
    message = %{
      subject: template |> Atom.to_string() |> String.replace("_", " ") |> String.capitalize(),
      to: {email_data.to_name, email_data.to_address},
      html: "",
      data: email_data
    }

    meta = %{token: Keyword.get(email_data.assigns || [], :token)}

    event =
      case template do
        :verification -> :on_mailer_record_verification_send
        :password_reset -> :on_mailer_record_password_reset_send
        _ -> :on_mailer_send
      end

    result =
      case event do
        :on_mailer_send ->
          MailerHooks.trigger_send(message, nil)

        _ ->
          MailerHooks.trigger_record_verification_send(message, record, meta, collection_name)
      end

    case result do
      {:ok, %{data: email_data}} -> {:ok, email_data}
      {:ok, _} -> {:ok, email_data}
      :skip -> :skip
      {:error, reason} -> {:error, reason}
    end
  end

  # ── Helpers ─────────────────────────────────────────────

  @doc false
  def run_after_funs(after_funs, event) do
    Registry.run_after(after_funs, event)
  end

  defp record_of(%Event{} = event), do: Event.get(event, :record) || %{}
end
