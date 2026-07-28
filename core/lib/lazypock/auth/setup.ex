defmodule Lazypock.Auth.Setup do
  @moduledoc """
  Boot-time setup for the superuser auth system.

  On application start:
  1. Creates the `_superusers` table if it doesn't exist
  2. Auto-creates a superuser from env vars if configured
  """

  alias Lazypock.Repo
  alias Lazypock.Auth.SuperUser

  @doc """
  Ensures the `_superusers` table exists.
  Called during application startup.
  """
  def ensure_superusers_table! do
    Ecto.Adapters.SQL.query!(
      Repo,
      """
      CREATE TABLE IF NOT EXISTS _superusers (
        id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
        email         TEXT UNIQUE NOT NULL,
        password_hash TEXT NOT NULL,
        created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
        updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
      )
      """,
      []
    )
  end

  @doc """
  Creates a superuser from environment variables if configured.
  """
  def create_from_env! do
    email = System.get_env("LAZYPOCK_SUPERUSER_EMAIL")
    password = System.get_env("LAZYPOCK_SUPERUSER_PASSWORD")

    if email && password && email != "" && password != "" do
      case Repo.get_by(SuperUser, email: email) do
        nil ->
          password_hash = Bcrypt.hash_pwd_salt(password)
          insert_superuser!(email, password_hash)
          :ok

        _ ->
          :already_exists
      end
    else
      :no_config
    end
  end

  @doc """
  Returns true if at least one superuser exists.
  """
  def any_superuser? do
    case Ecto.Adapters.SQL.query(Repo, "SELECT COUNT(*) FROM _superusers", []) do
      {:ok, %{rows: [[count]]}} -> count > 0
      _ -> false
    end
  end

  @doc """
  Creates the first superuser. Only works if no superusers exist.
  Uses raw SQL to avoid Ecto schema/table mismatch issues.
  """
  def create_first(email, password) do
    if any_superuser?() do
      {:error, "A superuser already exists. Use login instead."}
    else
      password_hash = Bcrypt.hash_pwd_salt(password)

      case Ecto.Adapters.SQL.query(
             Repo,
             "INSERT INTO _superusers (email, password_hash) VALUES ($1, $2) RETURNING id, email, created_at, updated_at",
             [email, password_hash]
           ) do
        {:ok, %{rows: [[id, email, created_at, updated_at]]}} ->
          now = DateTime.utc_now()

          superuser = %SuperUser{
            id: id,
            email: email,
            password_hash: password_hash,
            created_at: created_at || now,
            updated_at: updated_at || now
          }

          {:ok, superuser}

        {:error, err} ->
          {:error, Exception.message(err)}
      end
    end
  end

  @doc """
  Authenticates a superuser by email and password.
  """
  def authenticate(email, password) do
    case Repo.get_by(SuperUser, email: email) do
      nil ->
        {:error, "Invalid email or password"}

      superuser ->
        if Bcrypt.verify_pass(password, superuser.password_hash) do
          {:ok, superuser}
        else
          {:error, "Invalid email or password"}
        end
    end
  end

  defp insert_superuser!(email, password_hash) do
    Ecto.Adapters.SQL.query!(
      Repo,
      "INSERT INTO _superusers (email, password_hash) VALUES ($1, $2) ON CONFLICT (email) DO NOTHING",
      [email, password_hash]
    )
  end
end
