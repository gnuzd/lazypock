defmodule Lazypock.Mailer do
  @moduledoc """
  Swoosh-based mailer for sending transactional emails.

  SMTP configuration is read at send-time from `_settings.data.mail`,
  so users can change mail settings in the admin UI without restarts.
  """
  use Swoosh.Mailer, otp_app: :lazypock

  import Swoosh.Email

  alias Lazypock.Repo

  @doc """
  Builds and sends an email using the SMTP config from `_settings`.

  Returns `{:ok, result}` or `{:error, reason}`.
  """
  @spec deliver(
          template :: atom(),
          to_name :: String.t(),
          to_address :: String.t(),
          assigns :: keyword()
        ) ::
          {:ok, term()} | {:error, term()}
  def deliver(template, to_name, to_address, assigns \\ []) do
    sender_name = get_setting("mail", "sender_name") || "LazyPock"
    sender_address = get_setting("mail", "sender_address") || "noreply@lazypock.app"

    email =
      new()
      |> from({sender_name, sender_address})
      |> to({to_name, to_address})
      |> subject(subject_for(template, assigns))
      |> html_body(render_body(template, assigns))

    # Fire onMailerSend (PocketBase parity) — handlers can modify or drop
    case Lazypock.Hooks.Mailer.trigger_send(email) do
      {:ok, email} ->
        smtp_config = build_smtp_config()

        case Lazypock.Mailer.deliver(email, smtp_config) do
          {:ok, _result} -> :ok
          {:error, reason} -> {:error, reason}
        end

      :skip ->
        :ok

      {:error, reason} ->
        {:error, reason}
    end
  end

  @doc """
  Sends a pre-built `Swoosh.Email` struct, applying SMTP config from settings.
  Used by hooks that need full control over the email.
  """
  @spec deliver_email(Swoosh.Email.t()) :: {:ok, term()} | {:error, term()}
  def deliver_email(%Swoosh.Email{} = email) do
    smtp_config = build_smtp_config()
    Lazypock.Mailer.deliver(email, smtp_config)
  end

  # ── SMTP config from settings ──

  defp build_smtp_config do
    mail = get_settings()["mail"] || %{}
    enabled = Map.get(mail, "enabled", false)

    if enabled do
      [
        adapter: Swoosh.Adapters.SMTP,
        relay: Map.get(mail, "host") || "localhost",
        port: (Map.get(mail, "port") || "587") |> String.to_integer(),
        username: Map.get(mail, "user"),
        password: Map.get(mail, "pass"),
        tls: if(Map.get(mail, "tls", true), do: :always, else: :never),
        auth: Map.get(mail, "auth_method", "always"),
        retries: 1,
        no_mx_lookups: true
      ]
    else
      # Swoosh test adapter — logs instead of sending
      [adapter: Swoosh.Adapters.Test]
    end
  end

  # ── Subject lines ──

  defp subject_for(:verification, _assigns), do: "Verify your email address"
  defp subject_for(:password_reset, _assigns), do: "Password reset request"
  defp subject_for(:email_change, _assigns), do: "Confirm your new email address"

  defp subject_for(template, _assigns),
    do: template |> Atom.to_string() |> String.replace("_", " ") |> String.capitalize()

  # ── Template rendering ──

  defp render_body(template, assigns) do
    app_name = get_setting("application", "app_name") || "LazyPock"

    assigns = Keyword.put(assigns, :app_name, app_name)

    # Check for user override in priv/emails/
    template_path = template_path(template)

    if template_path && File.exists?(template_path) do
      EEx.eval_file(template_path, assigns: assigns)
    else
      render_default(template, assigns)
    end
  end

  defp template_path(template) do
    path = Application.app_dir(:lazypock, "priv/emails/#{template}.html.eex")

    if path && File.exists?(path) do
      path
    else
      # Fallback: check project root relative path (dev)
      dev_path = Path.join(File.cwd!(), "priv/emails/#{template}.html.eex")

      if File.exists?(dev_path) do
        dev_path
      else
        nil
      end
    end
  end

  # ── Default HTML templates ──

  defp render_default(:verification, assigns) do
    token = Keyword.get(assigns, :token, "")
    app_name = Keyword.get(assigns, :app_name, "LazyPock")

    ~s"""
    <!DOCTYPE html>
    <html>
    <head><meta charset="utf-8"></head>
    <body style="font-family: -apple-system, BlinkMacSystemFont, sans-serif; padding: 24px; max-width: 480px; margin: 0 auto;">
      <div style="text-align: center; margin-bottom: 32px;">
        <h1 style="font-size: 24px; font-weight: 600; color: #111;">Verify your email</h1>
        <p style="color: #666; font-size: 14px;">#{app_name}</p>
      </div>
      <p style="color: #333; font-size: 15px; line-height: 1.5;">
        Click the button below to verify your email address.
      </p>
      <div style="text-align: center; margin: 32px 0;">
        <a href="#{token}" style="display: inline-block; padding: 12px 24px; background-color: #2563eb; color: #fff; text-decoration: none; border-radius: 6px; font-size: 15px;">
          Verify Email
        </a>
      </div>
      <p style="color: #999; font-size: 12px;">
        If you didn't create an account, you can ignore this email.
      </p>
    </body>
    </html>
    """
  end

  defp render_default(:password_reset, assigns) do
    token = Keyword.get(assigns, :token, "")
    app_name = Keyword.get(assigns, :app_name, "LazyPock")

    ~s"""
    <!DOCTYPE html>
    <html>
    <head><meta charset="utf-8"></head>
    <body style="font-family: -apple-system, BlinkMacSystemFont, sans-serif; padding: 24px; max-width: 480px; margin: 0 auto;">
      <div style="text-align: center; margin-bottom: 32px;">
        <h1 style="font-size: 24px; font-weight: 600; color: #111;">Reset your password</h1>
        <p style="color: #666; font-size: 14px;">#{app_name}</p>
      </div>
      <p style="color: #333; font-size: 15px; line-height: 1.5;">
        We received a request to reset your password. Click the button below to choose a new one.
      </p>
      <div style="text-align: center; margin: 32px 0;">
        <a href="#{token}" style="display: inline-block; padding: 12px 24px; background-color: #2563eb; color: #fff; text-decoration: none; border-radius: 6px; font-size: 15px;">
          Reset Password
        </a>
      </div>
      <p style="color: #999; font-size: 12px;">
        If you didn't request a password reset, you can ignore this email.
      </p>
    </body>
    </html>
    """
  end

  defp render_default(:email_change, assigns) do
    token = Keyword.get(assigns, :token, "")
    new_email = Keyword.get(assigns, :new_email, "")
    app_name = Keyword.get(assigns, :app_name, "LazyPock")

    ~s"""
    <!DOCTYPE html>
    <html>
    <head><meta charset="utf-8"></head>
    <body style="font-family: -apple-system, BlinkMacSystemFont, sans-serif; padding: 24px; max-width: 480px; margin: 0 auto;">
      <div style="text-align: center; margin-bottom: 32px;">
        <h1 style="font-size: 24px; font-weight: 600; color: #111;">Confirm email change</h1>
        <p style="color: #666; font-size: 14px;">#{app_name}</p>
      </div>
      <p style="color: #333; font-size: 15px; line-height: 1.5;">
        Please confirm you want to change your email to <strong>#{new_email}</strong>.
      </p>
      <div style="text-align: center; margin: 32px 0;">
        <a href="#{token}" style="display: inline-block; padding: 12px 24px; background-color: #2563eb; color: #fff; text-decoration: none; border-radius: 6px; font-size: 15px;">
          Confirm Change
        </a>
      </div>
    </body>
    </html>
    """
  end

  # ── Helpers ──

  defp get_setting(category, key) do
    data = get_settings()
    category_map = Map.get(data, category) || %{}
    Map.get(category_map, key)
  end

  defp get_settings do
    case Ecto.Adapters.SQL.query(Repo, "SELECT data FROM _settings LIMIT 1", []) do
      {:ok, %{rows: [[data]]}} when is_map(data) ->
        data

      {:ok, %{rows: [[data]]}} when is_binary(data) ->
        Jason.decode!(data)

      _ ->
        %{}
    end
  end
end
