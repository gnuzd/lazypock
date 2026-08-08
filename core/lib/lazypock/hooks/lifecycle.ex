defmodule Lazypock.Hooks.Lifecycle do
  @moduledoc """
  **Deprecated** — use `Lazypock.Hooks.Hook` instead.

  Legacy behaviour for file-based collection lifecycle hooks. Kept for
  backwards compatibility: existing `priv/hooks/*.ex` modules using
  `use Lazypock.Hooks.Lifecycle, collection: "posts"` still work, but new
  code should use the PocketBase-parity `Lazypock.Hooks.Hook` macro.

  The legacy callbacks (`on_create/2`, `after_create/2`, `on_update/3`,
  `after_update/3`, `on_delete/2`, `after_delete/2`, `validate/2`,
  `on_email/2`) are automatically bridged to the new event-hook registry.
  """

  defmacro __using__(opts) do
    quote bind_quoted: [opts: opts] do
      @behaviour Lazypock.Hooks.Lifecycle

      @hook_collection Keyword.get(opts, :collection)

      @doc false
      def __collection__, do: @hook_collection

      def on_create(record, _ctx), do: {:ok, record}
      def after_create(_record, _ctx), do: :ok
      def on_update(_old, new_attrs, _ctx), do: {:ok, new_attrs}
      def after_update(_old, _new, _ctx), do: :ok
      def on_delete(_record, _ctx), do: :ok
      def after_delete(_record, _ctx), do: :ok
      def validate(_record, _ctx), do: :ok
      def on_email(email_data, _ctx), do: {:ok, email_data}

      defoverridable on_create: 2,
                     after_create: 2,
                     on_update: 3,
                     after_update: 3,
                     on_delete: 2,
                     after_delete: 2,
                     validate: 2,
                     on_email: 2

      @doc false
      def __hook_registrations__ do
        collection = @hook_collection

        [
          {:on_record_create, {__MODULE__, :__legacy_on_create__}, [collections: [collection]]},
          {:on_record_after_create_success, {__MODULE__, :__legacy_after_create__},
           [collections: [collection]]},
          {:on_record_update, {__MODULE__, :__legacy_on_update__}, [collections: [collection]]},
          {:on_record_after_update_success, {__MODULE__, :__legacy_after_update__},
           [collections: [collection]]},
          {:on_record_delete, {__MODULE__, :__legacy_on_delete__}, [collections: [collection]]},
          {:on_record_after_delete_success, {__MODULE__, :__legacy_after_delete__},
           [collections: [collection]]},
          {:on_record_validate, {__MODULE__, :__legacy_validate__}, [collections: [collection]]}
        ]
      end

      # ── Bridge functions (delegate to Lifecycle.bridge_* helpers) ──

      def __legacy_on_create__(event),
        do: Lazypock.Hooks.Lifecycle.bridge_on_create(__MODULE__, event, @hook_collection)

      def __legacy_after_create__(event),
        do: Lazypock.Hooks.Lifecycle.bridge_after_create(__MODULE__, event, @hook_collection)

      def __legacy_on_update__(event),
        do: Lazypock.Hooks.Lifecycle.bridge_on_update(__MODULE__, event, @hook_collection)

      def __legacy_after_update__(event),
        do: Lazypock.Hooks.Lifecycle.bridge_after_update(__MODULE__, event, @hook_collection)

      def __legacy_on_delete__(event),
        do: Lazypock.Hooks.Lifecycle.bridge_on_delete(__MODULE__, event, @hook_collection)

      def __legacy_after_delete__(event),
        do: Lazypock.Hooks.Lifecycle.bridge_after_delete(__MODULE__, event, @hook_collection)

      def __legacy_validate__(event),
        do: Lazypock.Hooks.Lifecycle.bridge_validate(__MODULE__, event, @hook_collection)
    end
  end

  # ── Bridge helpers (public so `apply/3` results are untyped) ──

  alias Lazypock.Hooks.Event

  @doc false
  def bridge_on_create(module, event, collection) do
    record = Event.get(event, :record) || %{}

    case apply(module, :on_create, [record, %{collection: collection}]) do
      {:ok, modified} -> Event.next(Event.put(event, :record, modified))
      {:error, reason} -> {:error, reason}
    end
  end

  @doc false
  def bridge_after_create(module, event, collection) do
    record = Event.get(event, :record) || %{}
    apply(module, :after_create, [record, %{collection: collection}])
    Event.next(event)
  end

  @doc false
  def bridge_on_update(module, event, collection) do
    old = Event.get(event, :old_record) || Event.get(event, :record)
    new = Event.get(event, :record) || %{}

    case apply(module, :on_update, [old, new, %{collection: collection}]) do
      {:ok, modified} -> Event.next(Event.put(event, :record, modified))
      {:error, reason} -> {:error, reason}
    end
  end

  @doc false
  def bridge_after_update(module, event, collection) do
    old = Event.get(event, :old_record)
    new = Event.get(event, :record)
    apply(module, :after_update, [old, new, %{collection: collection}])
    Event.next(event)
  end

  @doc false
  def bridge_on_delete(module, event, collection) do
    record = Event.get(event, :record) || %{}

    case apply(module, :on_delete, [record, %{collection: collection}]) do
      :ok -> Event.next(event)
      {:error, reason} -> {:error, reason}
    end
  end

  @doc false
  def bridge_after_delete(module, event, collection) do
    record = Event.get(event, :record) || %{}
    apply(module, :after_delete, [record, %{collection: collection}])
    Event.next(event)
  end

  @doc false
  def bridge_validate(module, event, collection) do
    record = Event.get(event, :record) || %{}

    case apply(module, :validate, [record, %{collection: collection}]) do
      :ok -> Event.next(event)
      {:error, _} -> {:error, "validation failed"}
    end
  end

  # ── Behaviour ───────────────────────────────────────────

  @type context :: %{
          optional(:conn) => term(),
          optional(:user) => map() | nil,
          optional(:collection) => map()
        }

  @doc "Called before a record is created."
  @callback on_create(map(), context()) :: {:ok, map()} | {:error, term()}

  @doc "Called after a record is created (fire-and-forget)."
  @callback after_create(map(), context()) :: :ok

  @doc "Called before a record is updated."
  @callback on_update(map(), map(), context()) :: {:ok, map()} | {:error, term()}

  @doc "Called after a record is updated (fire-and-forget)."
  @callback after_update(map(), map(), context()) :: :ok

  @doc "Called before a record is deleted."
  @callback on_delete(map(), context()) :: :ok | {:error, term()}

  @doc "Called after a record is deleted (fire-and-forget)."
  @callback after_delete(map(), context()) :: :ok

  @doc "Custom validation."
  @callback validate(map(), context()) :: :ok | {:error, keyword()}

  @doc "Intercept or customize an outgoing email."
  @callback on_email(map(), context()) :: {:ok, map()} | {:error, term()} | :skip

  @optional_callbacks [
    on_create: 2,
    after_create: 2,
    on_update: 3,
    after_update: 3,
    on_delete: 2,
    after_delete: 2,
    validate: 2,
    on_email: 2
  ]
end
