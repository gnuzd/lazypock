defmodule Lazypock.Hooks.Event do
  @moduledoc """
  The event chain model shared by every LazyPock hook — a faithful mirror of
  the PocketBase JS event hooks convention:

    * every handler has the same `function(e){}` signature
    * handlers call `e.next()` to proceed with the execution chain
    * throwing an error **or not calling** `e.next()` stops the chain

  Since LazyPock hooks are written in Elixir, the chain is expressed as
  return values instead of exceptions:

    * `{:ok, event}` — handler mutated `e` and wants to proceed (equivalent to
      PocketBase calling `e.next()` after its mutation)
    * `{:error, reason}` — handler aborts the chain (equivalent to PocketBase
      throwing inside a handler before `e.next()`)
    * `{:after, fun}` — handler ran its "before `e.next()`" logic, called
      `Event.next(e)` and wants its remaining code to run **after** the
      persistence/action step. This mirrors PocketBase's documented
      "Operations AFTER the `e.next()` execute after ..." semantics.

  ## Example

      def on_record_create(%Event{} = e) do
        # before e.next() — mutate the record
        e = Event.set(e, :record, Map.put(e.record, "slug", slugify(e.record["title"])))
        # call e.next() and schedule post-persist work
        Event.next(e, fn e ->
          Logger.info("record created: \#{e.record["id"]}")
          :ok
        end)
      end

  Handlers are registered per `event` and optionally scoped to a `collection`.
  """

  @type t :: %__MODULE__{
          event: atom(),
          app: term(),
          collection: term() | nil,
          collection_name: String.t() | nil,
          collection_name: String.t() | nil,
          record: term() | nil,
          data: map()
        }

  defstruct event: nil,
            app: nil,
            collection: nil,
            collection_name: nil,
            record: nil,
            data: %{}

  @doc "Returns the value stored under `key` in the event's data map."
  def get(%__MODULE__{data: data}, key), do: Map.get(data, key)

  @doc "Stores `value` under `key` in the event's data map (immutable copy)."
  @spec put(t(), atom(), term()) :: t()
  def put(%__MODULE__{data: data} = e, key, value) do
    e = %{e | data: Map.put(data, key, value)}
    if key == :record, do: %{e | record: value}, else: e
  end

  @doc "Convenience accessor for the `record` field (PocketBase `e.record`)."
  def record(%__MODULE__{} = e), do: get(e, :record)

  @doc "Convenience accessor for the `records` field (PocketBase `e.records`)."
  def records(%__MODULE__{} = e), do: get(e, :records)

  @doc "Convenience accessor for the `error` field (PocketBase `e.error`)."
  def error(%__MODULE__{} = e), do: get(e, :error)

  @doc """
  The PocketBase `e.next()` equivalent: proceed with the chain.

  When `post_fun` is given, the handler's remaining work runs **after** the
  action completes (mirroring PocketBase's "operations after `e.next()`").
  The `post_fun` receives the final event and must return `:ok` or
  `{:error, reason}` (an error aborts the whole request/operation).
  """
  def next(e, post_fun \\ nil)

  def next(%__MODULE__{} = e, nil), do: {:ok, e}

  def next(%__MODULE__{} = e, post_fun) when is_function(post_fun, 1),
    do: {:after, e, post_fun}

  @doc """
  Runs the `post_fun` scheduled by a handler after the action completed.
  Returns `:ok` or `{:error, reason}`.
  """
  def run_after(%__MODULE__{} = e, post_fun) when is_function(post_fun, 1) do
    case post_fun.(e) do
      :ok -> :ok
      {:ok, _} -> :ok
      {:error, reason} -> {:error, reason}
      other -> {:error, {:invalid_after_result, other}}
    end
  end
end
