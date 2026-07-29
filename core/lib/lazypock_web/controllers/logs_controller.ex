defmodule LazypockWeb.LogsController do
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

  def list(conn, params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_list(conn, params)
  end

  def show(conn, %{"id" => id}) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_show(conn, id)
  end

  def collections(conn, _params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_collections(conn)
  end

  def stats(conn, _params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_stats(conn)
  end

  def delete_logs(conn, params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_delete_logs(conn, params)
  end

  # ── Superuser-guarded implementations ──

  defp do_list(conn, params) do
    page = max(1, (params["page"] || "1") |> String.to_integer())
    per_page = min(200, max(1, (params["perPage"] || "50") |> String.to_integer()))
    offset = (page - 1) * per_page
    collection_filter = params["collection"]

    where_clause =
      if collection_filter && collection_filter != "" do
        "WHERE collection = $1"
      else
        ""
      end

    count_params = if collection_filter, do: [collection_filter], else: []
    total_params = count_params

    total =
      case Ecto.Adapters.SQL.query(
             Repo,
             "SELECT COUNT(*) FROM _request_logs #{where_clause}",
             total_params
           ) do
        {:ok, %{rows: [[count]]}} -> count
        _ -> 0
      end

    select_params =
      if collection_filter do
        [collection_filter, per_page, offset]
      else
        [per_page, offset]
      end

    order_dir = if params["order"] == "asc", do: "ASC", else: "DESC"

    select_where =
      if collection_filter && collection_filter != "" do
        "WHERE collection = $1 ORDER BY created_at #{order_dir} LIMIT $2 OFFSET $3"
      else
        "ORDER BY created_at #{order_dir} LIMIT $1 OFFSET $2"
      end

    items =
      case Ecto.Adapters.SQL.query(
             Repo,
             "SELECT id, method, path, status, duration, ip, user_agent, referer, collection, error, created_at FROM _request_logs #{select_where}",
             select_params
           ) do
        {:ok, result} ->
          Enum.map(result.rows, fn row ->
            [
              id,
              method,
              path,
              status,
              duration,
              ip,
              user_agent,
              referer,
              collection,
              error,
              created_at
            ] =
              row

            %{
              "id" => id |> maybe_uuid_string(),
              "method" => method,
              "path" => path,
              "status" => status,
              "duration" => duration,
              "ip" => ip,
              "user_agent" => user_agent,
              "referer" => referer,
              "collection" => collection,
              "error" => error,
              "created_at" => maybe_iso8601(created_at)
            }
          end)

        {:error, _} ->
          []
      end

    json(conn, %{
      page: page,
      perPage: per_page,
      totalItems: total,
      totalPages: max(1, ceil(total / per_page)),
      items: items
    })
  end

  defp do_show(conn, id) do
    id_bin = maybe_uuid_to_bin(id)

    case Ecto.Adapters.SQL.query(
           Repo,
           "SELECT id, method, path, status, duration, ip, user_agent, referer, collection, error, created_at FROM _request_logs WHERE id = $1",
           [id_bin]
         ) do
      {:ok,
       %{
         rows: [
           [
             id,
             method,
             path,
             status,
             duration,
             ip,
             user_agent,
             referer,
             collection,
             error,
             created_at
           ]
         ]
       }} ->
        json(conn, %{
          "id" => maybe_uuid_string(id),
          "method" => method,
          "path" => path,
          "status" => status,
          "duration" => duration,
          "ip" => ip,
          "user_agent" => user_agent,
          "referer" => referer,
          "collection" => collection,
          "error" => error,
          "created_at" => maybe_iso8601(created_at)
        })

      _ ->
        conn |> put_status(404) |> json(%{error: "Log entry not found"})
    end
  end

  defp do_collections(conn) do
    case Ecto.Adapters.SQL.query(
           Repo,
           "SELECT DISTINCT collection FROM _request_logs WHERE collection IS NOT NULL ORDER BY collection",
           []
         ) do
      {:ok, result} ->
        names = Enum.map(result.rows, fn [name] -> name end)
        json(conn, %{items: names})

      {:error, _} ->
        json(conn, %{items: []})
    end
  end

  defp do_stats(conn) do
    total =
      case Ecto.Adapters.SQL.query(
             Repo,
             "SELECT COUNT(*) FROM _request_logs",
             []
           ) do
        {:ok, %{rows: [[count]]}} -> count
        _ -> 0
      end

    # Hourly request volume for the last 24 hours (for chart)
    hourly =
      case Ecto.Adapters.SQL.query(
             Repo,
             "SELECT date_trunc('hour', created_at) AS hour, COUNT(*) AS cnt FROM _request_logs WHERE created_at > now() - interval '24 hours' GROUP BY hour ORDER BY hour",
             []
           ) do
        {:ok, result} ->
          Enum.map(result.rows, fn [hour, count] ->
            %{
              date: maybe_iso8601(hour),
              total: count
            }
          end)

        {:error, _} ->
          []
      end

    # Last 24h total
    last_24h = Enum.reduce(hourly, 0, fn %{total: t}, acc -> acc + t end)

    # Errors (status >= 400) in last 24h
    errors_24h =
      case Ecto.Adapters.SQL.query(
             Repo,
             "SELECT COUNT(*) FROM _request_logs WHERE created_at > now() - interval '24 hours' AND status >= 400",
             []
           ) do
        {:ok, %{rows: [[count]]}} -> count
        _ -> 0
      end

    # Avg duration in last 24h
    avg_duration =
      case Ecto.Adapters.SQL.query(
             Repo,
             "SELECT COALESCE(AVG(duration), 0) FROM _request_logs WHERE created_at > now() - interval '24 hours'",
             []
           ) do
        {:ok, %{rows: [[avg]]}} -> avg
        _ -> 0
      end

    json(conn, %{
      total: total,
      hourly: hourly,
      last_24h: last_24h,
      errors_24h: errors_24h,
      avg_duration: avg_duration
    })
  end

  defp do_delete_logs(conn, params) do
    if params["all"] == "true" do
      Ecto.Adapters.SQL.query!(Repo, "TRUNCATE _request_logs", [])
      json(conn, %{ok: true})
    else
      # Delete entries older than 7 days by default
      Ecto.Adapters.SQL.query!(
        Repo,
        "DELETE FROM _request_logs WHERE created_at < now() - interval '7 days'",
        []
      )

      json(conn, %{ok: true})
    end
  end

  defp maybe_uuid_string(bin) when is_binary(bin) and byte_size(bin) == 16 do
    Ecto.UUID.cast!(bin)
  end

  defp maybe_uuid_string(val), do: val

  defp maybe_uuid_to_bin(id) when is_binary(id) do
    case Ecto.UUID.dump(id) do
      {:ok, bin} -> bin
      :error -> id
    end
  end

  defp maybe_iso8601(%DateTime{} = dt), do: DateTime.to_iso8601(dt)
  defp maybe_iso8601(%NaiveDateTime{} = ndt), do: NaiveDateTime.to_iso8601(ndt)
  defp maybe_iso8601(val), do: val
end
