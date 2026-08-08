defmodule Lazypock.Hooks.Realtime do
  @moduledoc """
  PocketBase realtime hooks — fired for SSE/realtime client connections and
  messages.

  Full parity with the PocketBase docs:

    * `onRealtimeConnectRequest` — `e.client`, `e.idleTimeout` + all
      RequestEvent fields. Any execution after `e.next()` happens after the
      client disconnects.
    * `onRealtimeSubscribeRequest` — `e.client`, `e.subscriptions` + all
      RequestEvent fields. Used to validate/modify the submitted subscription
      change.
    * `onRealtimeMessageSend` — `e.client`, `e.message` + all original connect
      RequestEvent fields. Fired when sending an SSE message to a client.

  Handlers use the `function(e)` chain convention.
  """

  alias Lazypock.Hooks.Registry

  # ── Registration helpers ────────────────────────────────

  @doc "Registers an `on_realtime_connect_request` handler."
  def on_realtime_connect_request(fun, opts \\ []),
    do: register(:on_realtime_connect_request, fun, opts)

  @doc "Registers an `on_realtime_subscribe_request` handler."
  def on_realtime_subscribe_request(fun, opts \\ []),
    do: register(:on_realtime_subscribe_request, fun, opts)

  @doc "Registers an `on_realtime_message_send` handler."
  def on_realtime_message_send(fun, opts \\ []),
    do: register(:on_realtime_message_send, fun, opts)

  defp register(event, fun, opts) when is_function(fun, 1) do
    Registry.register(event, {:fun, fun}, opts)
    :ok
  end

  defp register(_event, _fun, _opts), do: {:error, :invalid_handler}

  # ── Trigger points (called by the framework) ────────────

  @doc "Fires `on_realtime_connect_request`."
  def trigger_connect(conn, client, idle_timeout \\ nil) do
    data = %{client: client, idleTimeout: idle_timeout, request: conn}

    case Registry.dispatch(:on_realtime_connect_request, data) do
      {:ok, event, after_funs} -> {:ok, event, after_funs}
      {:ok, event} -> {:ok, event, []}
      {:error, reason} -> {:error, reason}
    end
  end

  @doc "Fires `on_realtime_subscribe_request`."
  def trigger_subscribe(conn, client, subscriptions) do
    data = %{client: client, subscriptions: subscriptions, request: conn}

    case Registry.dispatch(:on_realtime_subscribe_request, data) do
      {:ok, event, after_funs} -> {:ok, event, after_funs}
      {:ok, event} -> {:ok, event, []}
      {:error, reason} -> {:error, reason}
    end
  end

  @doc "Fires `on_realtime_message_send`."
  def trigger_message_send(conn, client, message) do
    data = %{client: client, message: message, request: conn}

    case Registry.dispatch(:on_realtime_message_send, data) do
      {:ok, event, _after} -> {:ok, Lazypock.Hooks.Event.get(event, :message) || message}
      {:ok, event} -> {:ok, Lazypock.Hooks.Event.get(event, :message) || message}
      {:error, reason} -> {:error, reason}
    end
  end
end
