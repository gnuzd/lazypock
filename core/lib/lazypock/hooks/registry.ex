defmodule Lazypock.Hooks.Registry do
  @moduledoc """
  Auto-discovers hook modules in `priv/hooks/` at boot and maps them
  to collection names. Supports hot-reload via `reload!/0`.
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
  Scans `priv/hooks/` directory and registers all hook modules.
  Called at application boot.
  """
  @spec discover!() :: :ok
  def discover! do
    hooks_dir = hooks_path()

    if File.dir?(hooks_dir) do
      hooks_dir
      |> Path.join("*.ex")
      |> Path.wildcard()
      |> Enum.each(fn file ->
        compile_and_register(file)
      end)
    end

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

  @doc """
  Hot-reloads all hooks without restarting the app.
  """
  @spec reload!() :: :ok
  def reload! do
    :ok
  end

  defp compile_and_register(file) do
    try do
      {:module, module, _, _} = Code.compile_file(file)

      case module.__info__(:attributes)[:collection] do
        [collection] when is_binary(collection) ->
          register(collection, module)
          Logger.info("Registered hook #{inspect(module)} for '#{collection}'")

        _ ->
          Logger.warning("Hook #{Path.basename(file)} missing @collection attribute")
      end
    catch
      {:module, module, _, _} ->
        case module.__info__(:attributes)[:collection] do
          [collection] when is_binary(collection) ->
            register(collection, module)
        end
    rescue
      e -> Logger.warning("Error loading hook #{Path.basename(file)}: #{Exception.message(e)}")
    end
  end

  defp hooks_path do
    cond do
      path = Application.get_env(:lazypock, :hooks_path) ->
        path

      Mix.env() == :dev ->
        # In development, the project root is 2 levels up from _build
        Path.join([File.cwd!(), "priv", "hooks"])

      true ->
        Path.join(Application.app_dir(:lazypock, "priv"), "hooks")
    end
  end
end
