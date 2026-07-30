defmodule Lazypock.Emails do
  @moduledoc """
  Handles transactional email workflows: verification, password reset, email change.

  Uses `_otps` table (system collection) to store one-time password tokens.
  """

  alias Lazypock.Repo
  alias Lazypock.Mailer
  alias Lazypock.Collections.Registry
  alias Lazypock.Hooks.Dispatcher

  @token_bytes 32

  @doc """
  Sends a verification email to a user in an auth collection.

  Creates an OTP record, then dispatches the email (with hook interception).
  """
  def request_verification(collection_name, email) do
    with {:ok, collection} <- Registry.get(collection_name),
         :ok <- ensure_auth_collection!(collection),
         {:ok, user} <- find_user_by_email(collection_name, collection, email),
         {:ok, otp} <- create_otp(collection_name, user, email, :verification) do
      token = otp.raw_token

      email_data = %{
        template: :verification,
        to_name: user["name"] || email,
        to_address: email,
        assigns: [token: token]
      }

      case Dispatcher.dispatch_email(:verification, email_data, %{
             collection: collection,
             user: user
           }) do
        {:ok, modified} ->
          Mailer.deliver(
            modified.template,
            modified.to_name,
            modified.to_address,
            modified.assigns
          )

        {:error, reason} ->
          {:error, reason}

        :skip ->
          :ok
      end
    end
  end

  @doc """
  Confirms a verification token. Sets `verified: true` on the user record.
  """
  def confirm_verification(collection_name, token) do
    with {:ok, collection} <- Registry.get(collection_name),
         :ok <- ensure_auth_collection!(collection),
         {:ok, otp_record, user} <- verify_otp(collection_name, token) do
      # Mark user as verified
      user_id = user["id"]
      now = DateTime.utc_now()

      Ecto.Adapters.SQL.query!(
        Repo,
        "UPDATE #{quote_table(collection_name)} SET verified = true, verificationToken = NULL, updated_at = $1 WHERE id = $2",
        [now, user_id]
      )

      # Delete used OTP
      delete_otp(otp_record)

      verified_user = Map.put(user, "verified", true)
      password_field = find_password_field(collection)
      safe_user = Map.drop(verified_user, [password_field])

      {:ok, safe_user}
    end
  end

  @doc """
  Sends a password reset email to a user in an auth collection.
  """
  def request_password_reset(collection_name, email) do
    with {:ok, collection} <- Registry.get(collection_name),
         :ok <- ensure_auth_collection!(collection),
         {:ok, user} <- find_user_by_email(collection_name, collection, email),
         {:ok, otp} <- create_otp(collection_name, user, email, :password_reset) do
      token = otp.raw_token

      email_data = %{
        template: :password_reset,
        to_name: user["name"] || email,
        to_address: email,
        assigns: [token: token]
      }

      case Dispatcher.dispatch_email(:password_reset, email_data, %{
             collection: collection,
             user: user
           }) do
        {:ok, modified} ->
          Mailer.deliver(
            modified.template,
            modified.to_name,
            modified.to_address,
            modified.assigns
          )

        {:error, reason} ->
          {:error, reason}

        :skip ->
          :ok
      end
    end
  end

  @doc """
  Confirms a password reset token. Updates the user's password.
  """
  def confirm_password_reset(collection_name, token, new_password) do
    with {:ok, collection} <- Registry.get(collection_name),
         :ok <- ensure_auth_collection!(collection),
         {:ok, otp_record, user} <- verify_otp(collection_name, token) do
      user_id = user["id"]
      password_field = find_password_field(collection)
      password_hash = Bcrypt.hash_pwd_salt(new_password)
      now = DateTime.utc_now()

      Ecto.Adapters.SQL.query!(
        Repo,
        "UPDATE #{quote_table(collection_name)} SET #{quote_ident(password_field)} = $1, updated_at = $2 WHERE id = $3",
        [password_hash, now, user_id]
      )

      # Delete used OTP
      delete_otp(otp_record)

      {:ok, %{id: user_id}}
    end
  end

  # ── OTP management ──

  defp create_otp(collection_name, user, email, _purpose) do
    raw_token = :crypto.strong_rand_bytes(@token_bytes) |> Base.url_encode64(padding: false)
    hashed_token = Bcrypt.hash_pwd_salt(raw_token)
    now = DateTime.utc_now()
    user_id = user["id"]

    Ecto.Adapters.SQL.query!(
      Repo,
      """
      INSERT INTO _otps (collection_ref, record_ref, password_hash, sent_to, created_at, updated_at)
      VALUES ($1, $2, $3, $4, $5, $6)
      """,
      [collection_name, user_id, hashed_token, email, now, now]
    )

    {:ok, %{raw_token: raw_token, record_ref: user_id, collection_ref: collection_name}}
  end

  defp verify_otp(collection_name, raw_token) do
    # Clean up expired OTPs first
    Ecto.Adapters.SQL.query!(
      Repo,
      "DELETE FROM _otps WHERE collection_ref = $1 AND created_at < now() - interval '24 hours'",
      [collection_name]
    )

    # Find matching OTP records for this collection
    case Ecto.Adapters.SQL.query(
           Repo,
           "SELECT id, record_ref, password_hash, sent_to, created_at FROM _otps WHERE collection_ref = $1 ORDER BY created_at DESC",
           [collection_name]
         ) do
      {:ok, %{rows: []}} ->
        {:error, "Invalid or expired token"}

      {:ok, %{rows: rows, columns: cols}} ->
        otps = rows_to_maps(cols, rows)

        # Try each OTP (find the one matching this token)
        result =
          Enum.reduce_while(nil, otps, fn otp, _acc ->
            if Bcrypt.verify_pass(raw_token, otp["password_hash"]) do
              {:halt, {:ok, otp}}
            else
              {:cont, nil}
            end
          end)

        case result do
          {:ok, otp} ->
            # Verify not expired (24 hour expiry)
            created = otp["created_at"]
            now = DateTime.utc_now()

            if created && DateTime.diff(now, created) < 86_400 do
              # Fetch the user record
              user_id = otp["record_ref"]

              case Ecto.Adapters.SQL.query(
                     Repo,
                     "SELECT * FROM #{quote_table(collection_name)} WHERE id = $1",
                     [user_id]
                   ) do
                {:ok, %{rows: [row], columns: cols}} ->
                  user = row_to_map(cols, row)
                  {:ok, otp, user}

                _ ->
                  {:error, "User not found"}
              end
            else
              # Delete expired OTP
              Ecto.Adapters.SQL.query!(
                Repo,
                "DELETE FROM _otps WHERE id = $1",
                [otp["id"]]
              )

              {:error, "Token expired"}
            end

          nil ->
            {:error, "Invalid token"}
        end
    end
  end

  defp delete_otp(otp) do
    Ecto.Adapters.SQL.query!(Repo, "DELETE FROM _otps WHERE id = $1", [otp["id"]])
  end

  # ── Helpers ──

  defp ensure_auth_collection!(%{type: "auth"}), do: :ok
  defp ensure_auth_collection!(_), do: {:error, "Not an auth collection"}

  defp find_user_by_email(collection_name, collection, email) do
    email_field = find_email_field(collection)

    case Ecto.Adapters.SQL.query(
           Repo,
           "SELECT * FROM #{quote_table(collection_name)} WHERE #{quote_ident(email_field)} = $1",
           [email]
         ) do
      {:ok, %{rows: [row], columns: cols}} ->
        {:ok, row_to_map(cols, row)}

      {:ok, %{rows: []}} ->
        {:error, "Invalid request"}

      _ ->
        {:error, "Invalid request"}
    end
  end

  defp find_email_field(collection) do
    email_fields =
      (collection.fields || [])
      |> Enum.filter(fn f -> f.type == "email" end)
      |> Enum.map(fn f -> f.name end)

    case email_fields do
      [name | _] -> name
      [] -> "email"
    end
  end

  defp find_password_field(collection) do
    password_fields =
      (collection.fields || [])
      |> Enum.filter(fn f -> f.type == "password" end)
      |> Enum.map(fn f -> f.name end)

    case password_fields do
      [name | _] -> name
      [] -> "password_hash"
    end
  end

  defp quote_table(name), do: "\"#{name}\""
  defp quote_ident(name), do: "\"#{name}\""

  defp rows_to_maps(columns, rows) do
    Enum.map(rows, fn row ->
      Enum.zip(columns, row) |> Map.new()
    end)
  end

  defp row_to_map(columns, row) do
    Enum.zip(columns, row) |> Map.new()
  end
end
