defmodule Lazypock.Hooks.User do
  @moduledoc """
  User-editable hooks (PocketBase `pb_hooks` style).

  Hook files live in a **user-writable directory on disk** — `~/.lazypock/hooks/`
  by default (override with `LAZYPOCK_HOOKS_DIR`). They are Elixir `.ex` files
  that `use Lazypock.Hooks.Hook`, compiled at runtime on boot, and registered
  exactly like the built-in `priv/hooks/*.ex` files.

  Unlike the built-in hooks (compiled into the binary at build time), files in
  this directory can be added/edited AFTER a release — just restart the server
  (or call `Lazypock.Hooks.User.reload!/0`).

  ## Example

      # ~/.lazypock/hooks/posts_hooks.ex
      defmodule PostsHooks do
        use Lazypock.Hooks.Hook, collection: "posts"

        def on_record_create(e) do
          slug = e.record["title"] |> to_string() |> String.downcase()
          e = Lazypock.Hooks.Event.put(e, :record, Map.put(e.record, "slug", slug))
          Lazypock.Hooks.Event.next(e)
        end
      end

  A `post_hooks.ex` example is copied into the dir on first boot (only if the
  file doesn't already exist — user edits are never overwritten).
  """

  require Logger

  @builtin_hooks "priv/hooks"

  @doc "Returns the user hooks directory, creating it if needed."
  @spec dir() :: String.t()
  def dir do
    base =
      System.get_env("LAZYPOCK_HOOKS_DIR") ||
        Path.join(user_base(), "hooks")

    File.mkdir_p!(base)
    base
  end

  @doc "Returns true when hook loading is disabled via `LAZYPOCK_DISABLE_HOOKS`."
  @spec disabled?() :: boolean()
  def disabled? do
    System.get_env("LAZYPOCK_DISABLE_HOOKS") in ["1", "true"]
  end

  @doc """
  Copies built-in hook files from the release into the user hooks dir, without
  overwriting existing files. Returns the list of files copied.
  """
  @spec sync_builtins!() :: [String.t()]
  def sync_builtins! do
    src = Application.app_dir(:lazypock, @builtin_hooks)
    dest = dir()

    if File.dir?(src) do
      src
      |> File.ls!()
      |> Enum.filter(&String.ends_with?(&1, ".ex"))
      |> Enum.sort()
      |> Enum.flat_map(fn name ->
        target = Path.join(dest, name)

        if File.exists?(target) do
          []
        else
          File.cp!(Path.join(src, name), target)
          [target]
        end
      end)
    else
      []
    end
  end

  @doc """
  Compiles and registers all `.ex` files in the user hooks dir, then discovers
  them (mirrors `Lazypock.Hooks.Registry.discover!` for user files).

  Returns `:ok`. Per-file compile failures are logged but do not abort boot.
  """
  @spec load!() :: :ok
  def load! do
    if disabled?() do
      Logger.warning("LAZYPOCK_DISABLE_HOOKS is set — user hooks are NOT loaded.")
      :ok
    else
      hooks_dir = dir()
      sync_builtins!()

      Logger.warning(
        "Loading user hooks from #{hooks_dir} — hook code runs with full server " <>
          "privileges (see HOOKS_SECURITY.md). Set LAZYPOCK_DISABLE_HOOKS=1 to disable."
      )

      hooks_dir
      |> Path.join("*.ex")
      |> Path.wildcard()
      |> Enum.sort()
      |> Enum.each(fn path ->
        case Code.compile_file(path) do
          [{mod, _binary}] when is_atom(mod) ->
            if function_exported?(mod, :__hook_registrations__, 0) do
              mod.__hook_registrations__()
              |> Enum.each(fn {event, target, opts} ->
                Lazypock.Hooks.Registry.register(event, target, opts)
                Logger.info("Registered user hook #{inspect(mod)} for event #{inspect(event)}")
              end)
            else
              Logger.warning("User hook file did not produce a hook module: #{path}")
            end

          [] ->
            Logger.warning("User hook file compiled to no module: #{path}")

          _other ->
            Logger.warning("Unexpected compile result for #{path}")
        end
      end)

      :ok
    end
  rescue
    e ->
      Logger.error("Failed to load user hooks: #{Exception.message(e)}")
      :ok
  end

  defp user_base do
    System.get_env("LAZYPOCK_DATA_DIR") || Path.join(System.user_home!(), ".lazypock")
  end
end
