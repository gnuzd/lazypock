defmodule LazypockWeb.SettingsController do
  use LazypockWeb, :controller

  require Logger

  alias Lazypock.Repo

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

  def show(conn, _params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_show(conn)
  end

  def update(conn, params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_update(conn, params)
  end

  defp do_show(conn) do
    settings = get_settings()
    json(conn, mask_secrets(settings))
  end

  defp do_update(conn, params) do
    incoming = Map.drop(params, ["_method", "_csrf_token"])
    existing = get_settings()
    merged = Map.merge(existing, incoming)

    upsert_settings(merged)

    # If CORS origins changed, refresh the in-memory cache immediately.
    if Map.has_key?(incoming, "cors_origins") do
      Lazypock.CORS.refresh_origins()
    end

    # Fire onSettingsReload (PocketBase parity)
    Lazypock.Hooks.App.trigger_settings_reload(merged)

    json(conn, incoming)
  end

  # Force-refresh the CORS origins cache (called by the Studio UI after saving).
  def refresh_cors(conn, _params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_refresh_cors(conn)
  end

  defp do_refresh_cors(conn) do
    origins = Lazypock.CORS.refresh_origins()
    json(conn, %{ok: true, origins: origins})
  end

  # ── API Key management (generated from the Settings dashboard) ──
  # Keys are stored as a list, each with id/created/expires/revoked.
  # The raw key is shown exactly once at creation.

  # List all API keys (metadata only — never the raw key).
  def list_api_keys(conn, _params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: json(conn, %{items: Lazypock.Settings.list_api_keys()})
  end

  # Create a new API key. Optional `expiresInDays` body param.
  def generate_api_key(conn, params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_generate_api_key(conn, params)
  end

  defp do_generate_api_key(conn, params) do
    expires_in =
      case params["expiresInDays"] do
        days when is_integer(days) and days > 0 -> days
        _ -> nil
      end

    {key, meta} = Lazypock.Settings.create_api_key(expires_in)

    json(conn, %{
      api_key: key,
      item: meta,
      created_at: meta["created_at"],
      expires_at: meta["expires_at"]
    })
  end

  # Revoke an API key by id.
  def revoke_api_key(conn, %{"id" => id}) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_revoke_api_key(conn, id)
  end

  defp do_revoke_api_key(conn, id) do
    case Lazypock.Settings.revoke_api_key(id) do
      :ok -> json(conn, %{ok: true})
      :error -> conn |> put_status(404) |> json(%{error: "API key not found"})
    end
  end

  # Back-compat alias for the previous single-key GET.
  def get_api_key(conn, _params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: json(conn, %{items: Lazypock.Settings.list_api_keys()})
  end

  defp mask_secrets(data) do
    case Map.has_key?(data, "api_key") do
      true -> Map.put(data, "api_key", "<hashed>")
      false -> data
    end
  end

  defp get_settings do
    case Ecto.Adapters.SQL.query(
           Repo,
           "SELECT data FROM _settings LIMIT 1",
           []
         ) do
      {:ok, %{rows: [[data]]}} when is_map(data) ->
        data

      {:ok, %{rows: [[data]]}} when is_binary(data) ->
        Jason.decode!(data)

      {:ok, _} ->
        %{}

      {:error, _} ->
        %{}
    end
  end

  defp upsert_settings(data) do
    case Ecto.Adapters.SQL.query(
           Repo,
           "UPDATE _settings SET data = $1, updated_at = now() WHERE id = (SELECT id FROM _settings LIMIT 1)",
           [data]
         ) do
      {:ok, %{num_rows: 0}} ->
        Ecto.Adapters.SQL.query!(
          Repo,
          "INSERT INTO _settings (data) VALUES ($1)",
          [data]
        )

      _ ->
        :ok
    end
  end

  # ── SQL Console (read-only) ──

  def sql_query(conn, params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_sql_query(conn, params)
  end

  defp do_sql_query(conn, params) do
    sql = params["sql"]

    cond do
      is_nil(sql) or String.trim(sql) == "" ->
        conn |> put_status(400) |> json(%{error: "SQL query is required"})

      not safe_query?(sql) ->
        conn
        |> put_status(403)
        |> json(%{error: "Only SELECT, EXPLAIN, and WITH queries are allowed"})

      true ->
        case Ecto.Adapters.SQL.query(Repo, sql, []) do
          {:ok, %{columns: columns, rows: rows}} ->
            safe_rows =
              Enum.map(rows, fn row ->
                Enum.map(row, fn
                  nil ->
                    nil

                  val when is_binary(val) ->
                    if byte_size(val) == 16 do
                      # UUID raw binary from Postgrex
                      case Ecto.UUID.load(val) do
                        {:ok, str} -> str
                        :error -> inspect(val)
                      end
                    else
                      # Normal text string
                      if String.valid?(val), do: val, else: inspect(val)
                    end

                  %DateTime{} = dt ->
                    DateTime.to_iso8601(dt)

                  %NaiveDateTime{} = dt ->
                    NaiveDateTime.to_iso8601(dt)

                  %Date{} = d ->
                    Date.to_iso8601(d)

                  %Decimal{} = d ->
                    Decimal.to_string(d)

                  val when is_integer(val) ->
                    val

                  val when is_float(val) ->
                    val

                  val when is_boolean(val) ->
                    val

                  val ->
                    inspect(val)
                end)
              end)

            json(conn, %{
              columns: columns,
              rows: safe_rows,
              total: length(rows)
            })

          {:error, err} ->
            conn |> put_status(400) |> json(%{error: Exception.message(err)})
        end
    end
  end

  @safe_prefixes ["SELECT", "EXPLAIN", "WITH", "WITH RECURSIVE"]

  defp safe_query?(sql) do
    trimmed = String.trim(sql) |> String.upcase()

    Enum.any?(@safe_prefixes, fn prefix ->
      String.starts_with?(trimmed, prefix)
    end)
  end

  # ── Export all collections ──

  def export_all(conn, _params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_export(conn)
  end

  defp do_export(conn) do
    payload = Lazypock.Backup.export()
    collections = length(payload.collections)
    records = payload.collections |> Enum.map(&length(&1.records)) |> Enum.sum()

    Lazypock.Audit.record("export", actor(conn), collections: collections, records: records)

    json(conn, payload)
  end

  # Streaming NDJSON archive. A NEW route on purpose: `GET /api/export`'s JSON
  # shape is a published contract (docs/static/backup.schema.json), so the
  # archive format is additive rather than a change to that response.
  def export_archive(conn, _params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_export_archive(conn)
  end

  defp do_export_archive(conn) do
    path = Lazypock.Backup.temp_archive_path()

    case Lazypock.Backup.export_stream(dest: path) do
      {:ok, %{path: path, bytes: bytes, collections: collections, records: records}} ->
        Lazypock.Audit.record("export.archive", actor(conn),
          collections: collections,
          records: records,
          bytes: bytes
        )

        conn
        |> put_resp_content_type("application/zip")
        |> put_resp_header(
          "content-disposition",
          ~s(attachment; filename="#{Path.basename(path)}")
        )
        # send_file (not send_chunked) so the response carries Content-Length,
        # which is what gives the Studio a real download progress bar.
        |> send_file(200, path)

      {:error, reason} ->
        conn
        |> put_status(500)
        |> json(%{error: "export failed: #{inspect(reason)}"})
    end
  end

  # ── Import collections ──

  def import_all(conn, params) do
    conn = require_superuser!(conn)
    conn = if conn.halted, do: conn, else: require_password!(conn, params)
    if conn.halted, do: conn, else: do_import(conn, params)
  end

  # Lets the Studio warn BEFORE uploading a multi-GB archive: it answers whether
  # an automatic undo checkpoint is still possible and whether this looks like a
  # Neon host (informational only).
  def import_preflight(conn, _params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: json(conn, Lazypock.Backup.preflight())
  end

  defp do_import(conn, params) do
    delete_missing = truthy?(params["deleteMissing"])
    atomic = parse_atomic(params["atomic"])
    preflight = Lazypock.Backup.preflight()

    # Above the size threshold there is no automatic undo checkpoint, so the
    # operator has to opt in explicitly instead of silently losing rollback.
    if not preflight.undo_available and not truthy?(params["confirm"]) do
      conn
      |> put_status(409)
      |> json(%{
        requires_confirmation: true,
        reason: "large_import",
        message: large_import_warning(preflight),
        preflight: preflight
      })
    else
      result = run_import(params, delete_missing, atomic)

      Lazypock.Audit.record("import", actor(conn),
        imported: length(result.imported),
        errors: length(result.errors),
        deleteMissing: delete_missing,
        atomic: atomic,
        source: import_source(params),
        rolled_back: Map.get(result, :rolled_back, false)
      )

      json(conn, result)
    end
  end

  # `POST /api/import` accepts either the classic JSON body
  # ({"collections": [...], ...}) or a multipart upload of an NDJSON archive.
  # The multipart form is what removes the 8 MB/whole-file-in-JS-string wall.
  defp run_import(params, delete_missing, atomic) do
    case params["file"] do
      %Plug.Upload{path: path} ->
        Lazypock.Backup.restore_archive(path, delete_missing, atomic: atomic, snapshot: true)

      _ ->
        Lazypock.Backup.restore(params["collections"] || [], delete_missing,
          atomic: atomic,
          snapshot: true
        )
    end
  end

  defp import_source(%{"file" => %Plug.Upload{}}), do: "archive"
  defp import_source(_), do: "json"

  defp truthy?(value), do: value in [true, "true", "1", 1]

  defp parse_atomic("per_collection"), do: :per_collection
  defp parse_atomic("batch"), do: :batch
  defp parse_atomic("false"), do: false
  defp parse_atomic(false), do: false
  defp parse_atomic(_other), do: :batch

  defp large_import_warning(preflight) do
    base =
      "This import is large: the database is #{preflight.db_size_mb} MB, above the " <>
        "#{preflight.threshold_mb} MB automatic-undo threshold. No undo checkpoint will " <>
        "be taken, so there is no one-click rollback if the import goes wrong."

    if preflight.neon_hosted do
      base <>
        " This looks like a Neon-hosted database — you can create a branch or use Neon's " <>
        "point-in-time restore as an alternative safety net: " <>
        "https://neon.tech/docs/introduction/branching"
    else
      base
    end
  end

  # ── Import rollback (undo the last import/restore) ──

  def import_status(conn, _params) do
    conn = require_superuser!(conn)

    if conn.halted do
      conn
    else
      json(conn, %{
        snapshot: Lazypock.Backup.last_snapshot(),
        preflight: Lazypock.Backup.preflight()
      })
    end
  end

  def import_rollback(conn, params) do
    conn = require_superuser!(conn)
    conn = if conn.halted, do: conn, else: require_password!(conn, params)
    if conn.halted, do: conn, else: do_import_rollback(conn)
  end

  # Re-authentication for destructive operations: a valid bearer token is not
  # enough — the acting superuser must confirm the request with their password.
  defp require_password!(conn, params) do
    user = conn.assigns[:current_superuser]
    password = params["password"]

    cond do
      not is_binary(password) or password == "" ->
        reject_password(conn, "Password confirmation is required")

      Bcrypt.verify_pass(password, user.password_hash) ->
        conn

      true ->
        reject_password(conn, "Invalid password")
    end
  end

  defp reject_password(conn, message) do
    conn
    |> put_status(401)
    |> json(%{code: 401, message: message, data: %{}})
    |> halt()
  end

  defp actor(conn) do
    case conn.assigns[:current_superuser] do
      %{email: email} -> email
      _ -> nil
    end
  end

  defp do_import_rollback(conn) do
    case Lazypock.Backup.rollback() do
      {:ok, result} ->
        Lazypock.Audit.record("import.rollback", actor(conn),
          collections: length(result.imported),
          errors: length(result.errors)
        )

        json(conn, Map.put(result, :rolled_back, true))

      {:error, :no_snapshot} ->
        conn
        |> put_status(404)
        |> json(%{error: "There is no import snapshot to roll back to"})

      {:error, reason} ->
        rollback_error(conn, reason)
    end
  end

  # Split so both rollback error shapes stay handled and reachable.
  defp rollback_error(conn, %{errors: _} = result) do
    conn
    |> put_status(400)
    |> json(Map.put(result, :rolled_back, true))
  end

  defp rollback_error(conn, reason) do
    conn
    |> put_status(500)
    |> json(%{error: inspect(reason)})
  end

  # ── Send test email ──

  def send_test_email(conn, params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_send_test_email(conn, params)
  end

  defp do_send_test_email(conn, params) do
    to_address = params["to"]

    if is_nil(to_address) or to_address == "" do
      conn
      |> put_status(400)
      |> json(%{error: "Recipient email is required"})
    else
      case Lazypock.Mailer.deliver(:verification, to_address, to_address, token: "test-token") do
        :ok ->
          json(conn, %{success: true, message: "Test email sent to #{to_address}"})

        {:error, reason} ->
          conn
          |> put_status(500)
          |> json(%{error: "Failed to send email: #{reason}"})
      end
    end
  end
end
