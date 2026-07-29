defmodule LazypockWeb.EmailController do
  @moduledoc """
  Handles email-related auth endpoints: verification, password reset, email change.

  PocketBase-compatible API:
    POST /api/:collection/request-verification
    POST /api/:collection/confirm-verification
    POST /api/:collection/request-password-reset
    POST /api/:collection/confirm-password-reset
    POST /api/:collection/request-email-change
    POST /api/:collection/confirm-email-change
  """

  use LazypockWeb, :controller

  alias Lazypock.Emails
  alias Lazypock.Collections.Registry

  # ── Request verification email ──

  def request_verification(conn, %{"collection" => collection_name} = params) do
    email = params["email"]

    with {:ok, collection} <- Registry.get(collection_name),
         :ok <- ensure_auth_collection!(collection),
         {:ok, _user} <- find_user_by_email(collection_name, collection, email) do
      Emails.request_verification(collection_name, email)
      resp(conn, 204, "")
    else
      {:error, "Invalid request"} ->
        # Always return 204 to avoid revealing whether email exists
        resp(conn, 204, "")

      {:error, _reason} ->
        resp(conn, 204, "")
    end
  end

  # ── Confirm verification ──

  def confirm_verification(conn, %{"collection" => collection_name} = params) do
    token = params["token"]

    case Emails.confirm_verification(collection_name, token) do
      {:ok, user} ->
        json(conn, %{
          "record" => user
        })

      {:error, reason} ->
        conn
        |> put_status(400)
        |> json(%{"code" => 400, "message" => reason, "data" => %{}})
    end
  end

  # ── Request password reset ──

  def request_password_reset(conn, %{"collection" => collection_name} = params) do
    email = params["email"]

    with {:ok, collection} <- Registry.get(collection_name),
         :ok <- ensure_auth_collection!(collection),
         {:ok, _user} <- find_user_by_email(collection_name, collection, email) do
      Emails.request_password_reset(collection_name, email)
      resp(conn, 204, "")
    else
      _ ->
        resp(conn, 204, "")
    end
  end

  # ── Confirm password reset ──

  def confirm_password_reset(conn, %{"collection" => collection_name} = params) do
    token = params["token"]
    password = params["password"]
    password_confirm = params["passwordConfirm"]

    cond do
      is_nil(password) or password == "" ->
        conn
        |> put_status(400)
        |> json(%{"code" => 400, "message" => "Password is required", "data" => %{}})

      password != password_confirm ->
        conn
        |> put_status(400)
        |> json(%{"code" => 400, "message" => "Passwords do not match", "data" => %{}})

      byte_size(password) < 8 ->
        conn
        |> put_status(400)
        |> json(%{
          "code" => 400,
          "message" => "Password must be at least 8 characters",
          "data" => %{}
        })

      true ->
        case Emails.confirm_password_reset(collection_name, token, password) do
          {:ok, _result} ->
            resp(conn, 204, "")

          {:error, reason} ->
            conn
            |> put_status(400)
            |> json(%{"code" => 400, "message" => reason, "data" => %{}})
        end
    end
  end

  # ── Helpers ──

  defp ensure_auth_collection!(%{type: "auth"}), do: :ok
  defp ensure_auth_collection!(_), do: {:error, "Not an auth collection"}

  defp find_user_by_email(collection_name, collection, email) do
    alias Lazypock.Repo

    email_field = find_email_field(collection)

    case Ecto.Adapters.SQL.query(
           Repo,
           "SELECT * FROM #{quote_table(collection_name)} WHERE #{quote_ident(email_field)} = $1",
           [email]
         ) do
      {:ok, %{rows: [row], columns: cols}} ->
        {:ok, row_to_map(cols, row)}

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

  defp row_to_map(columns, row) do
    Enum.zip(columns, row) |> Map.new()
  end

  defp quote_table(name), do: "\"#{name}\""
  defp quote_ident(name), do: "\"#{name}\""
end
