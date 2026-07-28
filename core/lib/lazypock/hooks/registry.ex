defmodule Lazypock.Hooks.Registry do
  @moduledoc """
  Auto-discovers hook modules at boot and maps them to collection names.

  Hook `.ex` files in `priv/hooks/` are compiled via `elixirc_paths` in mix.exs,
  producing `.beam` files that ship with the release. This means hooks work
  in production WITHOUT needing the Elixir compiler.
  """

  require Logger

  @doc """
  Returns all hook modules registered for a collection.
  """
  @spec get(String.t()) :: [module()]
  def get(collection_name) do
    case :persistent_term.get({:lazypock_hooks, collection_name}, :not_found) do
      :not_found -> []
      modules when is_list(modules) -> modules
    end
  end

  @doc """
  Scans ebin directories for beam files and registers any lifecycle hook modules.
  Called at application boot. Works in both dev and production releases.
  """
  @spec discover!() :: :ok
  def discover! do
    beam_paths()
    |> Enum.each(fn path ->
      try do
        basename = Path.basename(path, ".beam")

        mod_name =
          basename |> String.trim_leading("Elixir.") |> String.split(".") |> Module.concat()

        case Code.ensure_loaded(mod_name) do
          {:module, ^mod_name} ->
            if function_exported?(mod_name, :__collection__, 0) do
              collection = mod_name.__collection__()
              register(collection, mod_name)
              Logger.info("Registered hook #{inspect(mod_name)} for '#{collection}'")
            end

          _ ->
            :ok
        end
      rescue
        _ -> :ok
      end
    end)

    :ok
  end

  @doc """
  Registers a hook module for a collection.
  """
  @spec register(String.t(), module()) :: :ok
  def register(collection_name, module) do
    existing = get(collection_name)
    :persistent_term.put({:lazypock_hooks, collection_name}, [module | existing])
    :ok
  end

  defp beam_paths do
    # Find the app's own ebin directory
    app_dir = Application.app_dir(:lazypock, "ebin")

    if File.dir?(app_dir) do
      Path.wildcard(Path.join(app_dir, "*.beam"))
    else
      []
    end
  end
end
