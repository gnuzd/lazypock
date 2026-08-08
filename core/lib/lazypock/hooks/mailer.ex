defmodule Lazypock.Hooks.Mailer do
  @moduledoc """
  PocketBase mailer hooks — fired when emails are sent.

  Full parity with the PocketBase docs:

    * `onMailerSend` — fired for every email sent via `$app.newMailClient()`.
      Event fields: `e.mailer`, `e.message` (the email message: `subject`,
      `to`, `from`, `html`, `text`, ...). Handlers can mutate `e.message`
      (e.g. change the subject) and call `e.next()`, or return
      `{:error, reason}` to abort, or `:skip` to silently drop the email.
    * `onMailerRecordAuthAlertSend` — device login auth alert email.
      Fields: `e.record`, `e.meta.info`.
    * `onMailerRecordPasswordResetSend` — `e.record`, `e.meta.token`.
    * `onMailerRecordVerificationSend` — `e.record`, `e.meta.token`.
    * `onMailerRecordEmailChangeSend` — `e.record`, `e.meta.token`, `e.meta.newEmail`.
    * `onMailerRecordOTPSend` — `e.record`, `e.meta.otpId`, `e.meta.password`.

  Handlers use the `function(e)` chain convention. The `:skip` return value
  (PocketBase has no direct equivalent — it's a LazyPock convenience for
  silently dropping the email) stops delivery.
  """

  alias Lazypock.Hooks.Registry

  # ── Registration helpers ────────────────────────────────

  @doc "Registers an `on_mailer_send` handler."
  def on_mailer_send(fun, opts \\ []), do: register(:on_mailer_send, fun, opts)

  @doc "Registers an `on_mailer_record_auth_alert_send` handler."
  def on_mailer_record_auth_alert_send(fun, opts \\ []),
    do: register(:on_mailer_record_auth_alert_send, fun, opts)

  @doc "Registers an `on_mailer_record_password_reset_send` handler."
  def on_mailer_record_password_reset_send(fun, opts \\ []),
    do: register(:on_mailer_record_password_reset_send, fun, opts)

  @doc "Registers an `on_mailer_record_verification_send` handler."
  def on_mailer_record_verification_send(fun, opts \\ []),
    do: register(:on_mailer_record_verification_send, fun, opts)

  @doc "Registers an `on_mailer_record_email_change_send` handler."
  def on_mailer_record_email_change_send(fun, opts \\ []),
    do: register(:on_mailer_record_email_change_send, fun, opts)

  @doc "Registers an `on_mailer_record_otp_send` handler."
  def on_mailer_record_otp_send(fun, opts \\ []),
    do: register(:on_mailer_record_otp_send, fun, opts)

  defp register(event, fun, opts) when is_function(fun, 1) do
    Registry.register(event, {:fun, fun}, opts)
    :ok
  end

  defp register(_event, _fun, _opts), do: {:error, :invalid_handler}

  # ── Trigger points (called by the framework) ────────────

  @doc """
  Fires `on_mailer_send` for every outgoing email.

  Returns `{:ok, message}` to proceed, `{:error, reason}` to abort, or
  `:skip` to silently drop the email.
  """
  def trigger_send(message, mailer \\ nil) do
    data = %{message: message, mailer: mailer}

    case Registry.dispatch(:on_mailer_send, data) do
      {:ok, event, _after} -> {:ok, Lazypock.Hooks.Event.get(event, :message) || message}
      {:ok, event} -> {:ok, Lazypock.Hooks.Event.get(event, :message) || message}
      {:error, :skip} -> :skip
      {:error, reason} -> {:error, reason}
    end
  end

  @doc "Fires `on_mailer_record_auth_alert_send`."
  def trigger_record_auth_alert_send(message, record, meta, collection_name \\ nil) do
    trigger_record(:on_mailer_record_auth_alert_send, message, record, meta, collection_name)
  end

  @doc "Fires `on_mailer_record_password_reset_send`."
  def trigger_record_password_reset_send(message, record, meta, collection_name \\ nil) do
    trigger_record(:on_mailer_record_password_reset_send, message, record, meta, collection_name)
  end

  @doc "Fires `on_mailer_record_verification_send`."
  def trigger_record_verification_send(message, record, meta, collection_name \\ nil) do
    trigger_record(:on_mailer_record_verification_send, message, record, meta, collection_name)
  end

  @doc "Fires `on_mailer_record_email_change_send`."
  def trigger_record_email_change_send(message, record, meta, collection_name \\ nil) do
    trigger_record(:on_mailer_record_email_change_send, message, record, meta, collection_name)
  end

  @doc "Fires `on_mailer_record_otp_send`."
  def trigger_record_otp_send(message, record, meta, collection_name \\ nil) do
    trigger_record(:on_mailer_record_otp_send, message, record, meta, collection_name)
  end

  defp trigger_record(event, message, record, meta, collection_name) do
    data = %{message: message, record: record, meta: meta}

    case Registry.dispatch(event, data, collection_name) do
      {:ok, ev, _after} -> {:ok, Lazypock.Hooks.Event.get(ev, :message) || message}
      {:ok, ev} -> {:ok, Lazypock.Hooks.Event.get(ev, :message) || message}
      {:error, :skip} -> :skip
      {:error, reason} -> {:error, reason}
    end
  end
end
