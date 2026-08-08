defmodule Lazypock.Hooks.App do
  @moduledoc """
  PocketBase app-level hooks — `onBootstrap`, `onSettingsReload`,
  `onBackupCreate`, `onBackupRestore`, `onTerminate` and `onBeforeServe`
  (custom API routes).

  These fire for the whole app (no collection filter).

  ## Example

      Lazypock.Hooks.App.on_before_serve(fn e ->
        e = Lazypock.Hooks.Event.put(e, :routes, [{"GET", "/hello", fn conn -> ... end}]))
        Lazypock.Hooks.Event.next(e)
      end)
  """

  alias Lazypock.Hooks.Event
  alias Lazypock.Hooks.Registry

  @doc "Registers an `on_bootstrap` handler."
  def on_bootstrap(fun), do: register_app(:on_bootstrap, fun)

  @doc "Registers an `on_settings_reload` handler."
  def on_settings_reload(fun), do: register_app(:on_settings_reload, fun)

  @doc "Registers an `on_backup_create` handler."
  def on_backup_create(fun), do: register_app(:on_backup_create, fun)

  @doc "Registers an `on_backup_restore` handler."
  def on_backup_restore(fun), do: register_app(:on_backup_restore, fun)

  @doc "Registers an `on_terminate` handler."
  def on_terminate(fun), do: register_app(:on_terminate, fun)

  @doc "Registers an `on_before_serve` handler (custom API routes)."
  def on_before_serve(fun), do: register_app(:on_before_serve, fun)

  # ── Trigger points (called by the framework) ──────────────

  @doc "Fires `on_bootstrap`. Called once at application boot."
  def trigger_bootstrap(app \\ nil) do
    Registry.dispatch(:on_bootstrap, %{app: app || Lazypock.app()})
  end

  @doc "Fires `on_settings_reload`. Called whenever settings are replaced."
  def trigger_settings_reload(new_settings, app \\ nil) do
    Registry.dispatch(:on_settings_reload, %{app: app || Lazypock.app(), settings: new_settings})
  end

  @doc "Fires `on_backup_create`."
  def trigger_backup_create(app, name, exclude) do
    Registry.dispatch(:on_backup_create, %{app: app, name: name, exclude: exclude})
  end

  @doc "Fires `on_backup_restore`."
  def trigger_backup_restore(app, name, exclude) do
    Registry.dispatch(:on_backup_restore, %{app: app, name: name, exclude: exclude})
  end

  @doc "Fires `on_terminate`."
  def trigger_terminate(app, is_restart \\ false) do
    Registry.dispatch(:on_terminate, %{app: app, is_restart: is_restart})
  end

  @doc """
  Fires `on_before_serve`. Returns the list of `{method, path, handler}`
  custom routes registered by user hooks (or `[]`).
  """
  def trigger_before_serve(app) do
    case Registry.dispatch(:on_before_serve, %{app: app, routes: []}) do
      {:ok, %Event{} = ev} -> Event.get(ev, :routes) || []
      {:error, _} -> []
    end
  end

  defp register_app(event, fun) when is_function(fun, 1) do
    Registry.register(event, {:fun, fun}, collections: :any)
    :ok
  end

  defp register_app(_event, _fun), do: {:error, :invalid_handler}
end
