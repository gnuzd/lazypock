defmodule LazypockWeb.SettingsController do
  use LazypockWeb, :controller

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

    # Fire onSettingsReload (PocketBase parity)
    Lazypock.Hooks.App.trigger_settings_reload(merged)

    json(conn, incoming)
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
    collections =
      Lazypock.Collections.Registry.list()
      |> Enum.map(fn coll ->
        records = Lazypock.Schemas.GenericRecord.all(coll.name)

        %{
          name: coll.name,
          type: coll.type,
          schema: coll.schema,
          rules: coll.rules,
          options: coll.options,
          records: records
        }
      end)

    json(conn, %{collections: collections})
  end

  # ── Import collections ──

  def import_all(conn, params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_import(conn, params)
  end

  defp do_import(conn, params) do
    collections = params["collections"] || []
    delete_missing = params["deleteMissing"] == true

    existing_names =
      Lazypock.Collections.Registry.list()
      |> Enum.map(& &1.name)
      |> MapSet.new()

    incoming_names = Enum.map(collections, & &1["name"]) |> MapSet.new()

    {imported_list, errors_list} =
      Enum.reduce(collections, {[], []}, fn coll_data, {imported_acc, errors_acc} ->
        name = coll_data["name"]
        type = coll_data["type"] || "base"
        schema = coll_data["schema"] || []
        records = coll_data["records"] || []

        result =
          if name in existing_names do
            # Update existing collection — apply new schema fields
            case Lazypock.Schema.DDL.update_collection(name, fields: schema) do
              {:ok, _} -> {:ok, :updated}
              other -> other
            end
          else
            Lazypock.Schema.DDL.create_collection(name, type: type, fields: schema)
          end

        case result do
          {:ok, _} ->
            inserted =
              Enum.reduce_while(records, {:ok, 0}, fn record, {:ok, count} ->
                record_without_id = Map.drop(record, ["id", "created_at", "updated_at"])

                case Lazypock.Schemas.GenericRecord.insert(name, record_without_id) do
                  {:ok, _} -> {:cont, {:ok, count + 1}}
                  {:error, _reason} -> {:cont, {:ok, count}}
                end
              end)

            insert_count =
              case inserted do
                {:ok, c} -> c
                _ -> 0
              end

            {[%{name: name, type: type, records_imported: insert_count} | imported_acc],
             errors_acc}

          {:error, reason} ->
            {imported_acc, [%{name: name, error: reason} | errors_acc]}
        end
      end)

    # Delete missing collections if requested
    if delete_missing do
      for name <- MapSet.difference(existing_names, incoming_names) do
        case Lazypock.Schema.DDL.drop_collection(name) do
          {:ok, _} -> :ok
          {:error, _} -> :ok
        end
      end
    end

    json(conn, %{
      imported: Enum.reverse(imported_list),
      errors: Enum.reverse(errors_list)
    })
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
