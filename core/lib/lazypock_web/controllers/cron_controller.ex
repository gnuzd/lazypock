defmodule LazypockWeb.CronController do
  @moduledoc """
  Admin API for persisted cron jobs (`_crons`).

  PocketBase-parity surface: `GET /api/crons` lists jobs and
  `POST /api/crons/:id` triggers a job immediately. LazyPock extends it with
  full CRUD plus a `POST /api/crons/validate` helper used by the Studio to
  preview next run times client-side. All actions require a superuser token.
  """

  use LazypockWeb, :controller

  alias Lazypock.Cron

  defp require_superuser!(conn) do
    case conn.assigns[:current_superuser] do
      nil ->
        conn
        |> put_resp_content_type("application/json")
        |> send_resp(
          403,
          Jason.encode!(%{code: 403, message: "Access denied. Superuser required.", data: %{}})
        )
        |> halt()

      _user ->
        conn
    end
  end

  # ── List ───────────────────────────────────────────

  def index(conn, _params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: json(conn, %{items: Cron.list()})
  end

  # ── Create ─────────────────────────────────────────

  def create(conn, params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_create(conn, params)
  end

  defp do_create(conn, params) do
    case Cron.create(params) do
      {:ok, job} -> conn |> put_status(201) |> json(job)
      {:error, message} -> conn |> put_status(400) |> json(%{error: message})
    end
  end

  # ── Show ───────────────────────────────────────────

  def show(conn, %{"id" => id}) do
    conn = require_superuser!(conn)

    if conn.halted do
      conn
    else
      case Cron.get(id) do
        nil -> conn |> put_status(404) |> json(%{error: "Cron job not found"})
        job -> json(conn, job)
      end
    end
  end

  # ── Update ─────────────────────────────────────────

  def update(conn, %{"id" => id} = params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_update(conn, id, params)
  end

  defp do_update(conn, id, params) do
    case Cron.update(id, params) do
      {:ok, job} ->
        json(conn, job)

      {:error, :not_found} ->
        conn |> put_status(404) |> json(%{error: "Cron job not found"})

      {:error, message} ->
        conn |> put_status(400) |> json(%{error: message})
    end
  end

  # ── Delete ─────────────────────────────────────────

  def delete(conn, %{"id" => id}) do
    conn = require_superuser!(conn)

    if conn.halted do
      conn
    else
      case Cron.delete(id) do
        :ok -> json(conn, %{})
        :error -> conn |> put_status(404) |> json(%{error: "Cron job not found"})
      end
    end
  end

  # ── Run now (PocketBase parity: POST /api/crons/:id) ──

  def run(conn, %{"id" => id}) do
    conn = require_superuser!(conn)

    if conn.halted do
      conn
    else
      case Cron.run_now(id) do
        {:ok, job} -> json(conn, job)
        {:error, :not_found} -> conn |> put_status(404) |> json(%{error: "Cron job not found"})
      end
    end
  end

  # ── Validate / preview ─────────────────────────────

  def validate(conn, params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_validate(conn, params)
  end

  defp do_validate(conn, params) do
    expression = params["expression"]
    timezone = params["timezone"] || "UTC"

    case Cron.parse_expression(expression) do
      {:error, message} ->
        json(conn, %{valid: false, error: message})

      {:ok, _parsed} ->
        if Cron.valid_timezone?(timezone) do
          next_runs =
            expression
            |> Cron.next_run_dates(timezone, 5)
            |> Enum.map(&DateTime.to_iso8601/1)

          json(conn, %{valid: true, nextRuns: next_runs})
        else
          json(conn, %{valid: false, error: "Unknown timezone \"#{timezone}\""})
        end
    end
  end
end
