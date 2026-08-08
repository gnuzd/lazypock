defmodule Lazypock.Hooks.Registry do
  @moduledoc """
  Registry of hook handlers, mirroring the PocketBase event hooks model.

  Handlers are registered for an `event` (e.g. `:on_record_create`) and
  optionally scoped to a `collection` (PocketBase's trailing
  `onRecordCreate((e) => {...}, "users", "articles")` arguments).

  A handler target is either:

    * `{module, function_atom}` — a module exporting the handler function
      (used by `use Lazypock.Hooks.Hook` modules)
    * `{:fun, fun}` — an anonymous function of arity 1 (used by the
      convenience registration functions like `Lazypock.Hooks.App.on_bootstrap/1`)

  ## Registration

      # fires for every event of this type (PocketBase: no collection args)
      Lazypock.Hooks.Registry.register(:on_record_create, {MyHooks, :on_record_create})

      # fires only for "users" and "articles" records
      Lazypock.Hooks.Registry.register(:on_record_create, {MyHooks, :on_record_create},
        collections: ["users", "articles"])

  ## Execution chain

  `dispatch/4` runs handlers in registration order. Each handler receives a
  `Lazypock.Hooks.Event` and must return:

    * `{:ok, event}` — proceed (equivalent to PocketBase `e.next()`)
    * `{:error, reason}` — abort the chain
    * `{:after, event, fun}` — proceed and run `fun` after the action completes
      (PocketBase "operations AFTER `e.next()`")

  Throwing inside a handler is caught and converted to `{:error, ...}` —
  matching PocketBase's "throwing an error ... stops the hook execution chain".
  """

  require Logger

  alias Lazypock.Hooks.Event

  @key {:lazypock_hooks_registry, :handlers}

  @type target :: {module(), atom()} | {:fun, (Event.t() -> term())}

  @doc """
  Registers a hook handler for `event`.

  ## Options

    * `:collections` — list of collection names this handler fires for.
      `nil` (default) fires for every collection, matching PocketBase.

    * `:priority` — lower runs first (default `0`). PocketBase runs
      handlers in registration order; priorities let advanced users
      override that without depending on load order.
  """
  @spec register(atom(), target(), keyword()) :: :ok
  def register(event, target, opts \\ []) when is_atom(event) do
    collections = Keyword.get(opts, :collections)
    priority = Keyword.get(opts, :priority, 0)

    updated =
      all() ++
        [
          {event, target, collections, priority, System.unique_integer([:positive])}
        ]

    :persistent_term.put(@key, updated)
    :ok
  end

  @doc """
  Removes all handlers. Used by tests and on hot-reload.
  """
  @spec clear() :: :ok
  def clear do
    :persistent_term.erase(@key)
    :ok
  end

  @doc """
  Runs the execution chain for `event`.

  Returns:

    * `{:ok, event}` — all handlers proceeded (`e.next()` called everywhere)
    * `{:error, reason}` — a handler aborted the chain
    * `{:ok, event, after_funs}` — proceeded handlers that scheduled
      post-action work (PocketBase "operations AFTER `e.next()`")
  """
  @spec dispatch(atom(), map(), String.t() | nil, keyword()) ::
          {:ok, Event.t()} | {:error, term()} | {:ok, Event.t(), [function()]}
  def dispatch(event, data, collection_name \\ nil, opts \\ []) do
    collect_only = Keyword.get(opts, :collect_only, false)

    handlers =
      all()
      |> Enum.filter(fn {ev, _target, colls, _prio, _seq} ->
        ev == event and
          (colls == :any or is_nil(colls) or
             (is_list(colls) and not is_nil(collection_name) and collection_name in colls))
      end)
      |> Enum.sort_by(fn {_ev, _target, _colls, prio, seq} -> {prio, seq} end)
      |> Enum.map(fn {_ev, target, _colls, _prio, _seq} -> target end)

    if collect_only do
      # Call each handler, collecting successful results (skipping aborts).
      results =
        handlers
        |> Enum.reduce([], fn target, acc ->
          event = new_event(event, data, collection_name)

          case safe_call(target, event) do
            {:ok, %Event{} = ev} -> acc ++ [ev]
            _ -> acc
          end
        end)

      case results do
        [] -> {:ok, new_event(event, data, collection_name)}
        [last | _] -> {:ok, last}
      end
    else
      initial = new_event(event, data, collection_name)

      result =
        Enum.reduce_while(handlers, {:ok, initial, []}, fn target, {:ok, event, after_funs} ->
          case safe_call(target, event) do
            {:ok, %Event{} = ev} ->
              {:cont, {:ok, ev, after_funs}}

            {:after, %Event{} = ev, post_fun} ->
              {:cont, {:ok, ev, after_funs ++ [post_fun]}}

            {:error, reason} ->
              {:halt, {:error, reason}}

            other ->
              {:halt, {:error, {:invalid_hook_result, target, other}}}
          end
        end)

      # Contract: {:ok, event} when no after-work was scheduled,
      # {:ok, event, after_funs} when there was.
      case result do
        {:ok, event, []} -> {:ok, event}
        {:ok, event, after_funs} -> {:ok, event, after_funs}
        other -> other
      end
    end
  end

  @doc """
  Runs the post-action functions collected during `dispatch/4`.
  Each must return `:ok`; the first `{:error, reason}` aborts.
  """
  @spec run_after([function()], Event.t()) :: :ok | {:error, term()}
  def run_after(after_funs, event) do
    Enum.reduce_while(after_funs, :ok, fn fun, :ok ->
      case Event.run_after(event, fun) do
        :ok -> {:cont, :ok}
        {:error, reason} -> {:halt, {:error, reason}}
      end
    end)
  end

  @doc """
  Returns all handlers registered for `event` (optionally narrowed to
  `collection`) as `{event, target, collections}` tuples.
  """
  @spec get(atom(), String.t() | nil) :: [{atom(), target(), [String.t()] | nil}]
  def get(event, collection \\ nil) when is_atom(event) do
    all()
    |> Enum.filter(fn {ev, _target, colls, _prio, _seq} ->
      ev == event and
        (colls == :any or is_nil(colls) or
           (is_list(colls) and not is_nil(collection) and collection in colls))
    end)
    |> Enum.map(fn {ev, target, colls, _prio, _seq} -> {ev, target, colls} end)
  end

  @doc "Returns the raw handler list (for tests/introspection)."
  @spec all() :: [{atom(), target(), [String.t()] | nil, integer(), integer()}]
  def all do
    case :persistent_term.get(@key, []) do
      handlers when is_list(handlers) -> handlers
      _ -> []
    end
  end

  # ── Event construction ──────────────────────────────────

  # Builds the initial event, mirroring `data[:record]` into the `record`
  # struct field so handlers can use PocketBase-style `e.record` dot access.
  defp new_event(event, data, collection_name) do
    %Event{
      event: event,
      data: data,
      collection_name: collection_name,
      record: Map.get(data || %{}, :record)
    }
  end

  # ── Handler invocation ───────────────────────────────────

  # Calls a handler, catching raises so a throwing handler stops the chain
  # (PocketBase parity) instead of crashing the request process.
  defp safe_call({:fun, fun}, event) when is_function(fun, 1) do
    try do
      fun.(event)
    rescue
      e -> {:error, {:hook_exception, fun, e}}
    catch
      kind, reason -> {:error, {:hook_throw, kind, reason}}
    end
  end

  defp safe_call({mod, fun}, event) do
    try do
      apply(mod, fun, [event])
    rescue
      e -> {:error, {:hook_exception, mod, fun, e}}
    catch
      kind, reason -> {:error, {:hook_throw, kind, reason}}
    end
  end

  # ── Discovery (boot-time) ────────────────────────────────

  @doc """
  Scans `priv/hooks/` beam files and registers any module using
  `Lazypock.Hooks.Hook` (or the legacy `Lazypock.Hooks.Lifecycle`).
  Called at application boot.
  """
  @spec discover!() :: :ok
  def discover! do
    try do
      beam_paths()
      |> Enum.each(fn path ->
        basename = Path.basename(path, ".beam")

        mod_name =
          basename |> String.trim_leading("Elixir.") |> String.split(".") |> Module.concat()

        case Code.ensure_loaded(mod_name) do
          {:module, ^mod_name} ->
            if function_exported?(mod_name, :__hook_registrations__, 0) do
              # New-style hook module: self-registers via the __using__ macro.
              mod_name.__hook_registrations__()
              |> Enum.each(fn {event, target, opts} ->
                register(event, target, opts)
                Logger.info("Registered hook #{inspect(mod_name)} for event #{inspect(event)}")
              end)
            else
              if function_exported?(mod_name, :__collection__, 0) do
                # Legacy lifecycle module.
                register_legacy(mod_name)
              end
            end

          _ ->
            :ok
        end
      end)
    rescue
      _ -> :ok
    end

    :ok
  end

  @doc false
  def register_legacy(module) do
    collection = module.__collection__()
    Logger.debug("Registering legacy hook module #{inspect(module)} for '#{collection}'")

    register(:on_record_create, {module, :on_create}, collections: [collection])
    register(:on_record_after_create_success, {module, :after_create}, collections: [collection])
    register(:on_record_update, {module, :on_update}, collections: [collection])
    register(:on_record_after_update_success, {module, :after_update}, collections: [collection])
    register(:on_record_delete, {module, :on_delete}, collections: [collection])
    register(:on_record_after_delete_success, {module, :after_delete}, collections: [collection])
    register(:on_record_validate, {module, :validate}, collections: [collection])
    register(:on_mailer_record_verification_send, {module, :on_email}, collections: [collection])

    :ok
  end

  defp beam_paths do
    app_dir = Application.app_dir(:lazypock, "ebin")

    if File.dir?(app_dir) do
      Path.wildcard(Path.join(app_dir, "*.beam"))
    else
      []
    end
  end
end
