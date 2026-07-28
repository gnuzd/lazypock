defmodule Lazypock.Hooks.Lifecycle do
  @moduledoc """
  Behaviour for file-based collection lifecycle hooks.

  Users create `.ex` files in `priv/hooks/` that implement this behaviour.

  ## Example

      # priv/hooks/posts_hooks.ex
      defmodule MyApp.Hooks.PostsHooks do
        use Lazypock.Hooks.Lifecycle, collection: "posts"

        @impl true
        def on_create(record, context) do
          record = Map.put(record, :slug, Slug.slugify(record.title))
          {:ok, record}
        end
      end
  """

  @type context :: %{
          optional(:conn) => term(),
          optional(:user) => map() | nil,
          optional(:collection) => map()
        }

  @doc """
  Called before a record is created.
  Return `{:ok, modified_record}` to proceed, `{:error, reason}` to abort.
  """
  @callback on_create(map(), context()) :: {:ok, map()} | {:error, term()}

  @doc """
  Called after a record is created (fire-and-forget).
  """
  @callback after_create(map(), context()) :: :ok

  @doc """
  Called before a record is updated.
  Receives old record + new attrs.
  Return `{:ok, modified_attrs}` or `{:error, reason}`.
  """
  @callback on_update(map(), map(), context()) :: {:ok, map()} | {:error, term()}

  @doc """
  Called after a record is updated (fire-and-forget).
  """
  @callback after_update(map(), map(), context()) :: :ok

  @doc """
  Called before a record is deleted.
  Return `:ok` to proceed, `{:error, reason}` to abort.
  """
  @callback on_delete(map(), context()) :: :ok | {:error, term()}

  @doc """
  Called after a record is deleted (fire-and-forget).
  """
  @callback after_delete(map(), context()) :: :ok

  @doc """
  Custom validation. Return `:ok` or `{:error, field => message}`.
  """
  @callback validate(map(), context()) :: :ok | {:error, keyword()}

  @optional_callbacks [
    on_create: 2,
    after_create: 2,
    on_update: 3,
    after_update: 3,
    on_delete: 2,
    after_delete: 2,
    validate: 2
  ]

  defmacro __using__(opts) do
    collection = Keyword.fetch!(opts, :collection)

    quote do
      @behaviour Lazypock.Hooks.Lifecycle

      @doc false
      def __collection__, do: unquote(collection)

      def on_create(record, _ctx), do: {:ok, record}
      def after_create(_record, _ctx), do: :ok
      def on_update(_old, new_attrs, _ctx), do: {:ok, new_attrs}
      def after_update(_old, _new, _ctx), do: :ok
      def on_delete(_record, _ctx), do: :ok
      def after_delete(_record, _ctx), do: :ok
      def validate(_record, _ctx), do: :ok

      defoverridable on_create: 2,
                     after_create: 2,
                     on_update: 3,
                     after_update: 3,
                     on_delete: 2,
                     after_delete: 2,
                     validate: 2
    end
  end
end
