defmodule LazypockWeb.LogsController do
  use LazypockWeb, :controller

  alias Lazypock.Logs
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

  def retention(conn, _params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: json(conn, %{days: Logs.retention_days()})
  end

  def update_retention(conn, params) do
    conn = require_superuser!(conn)

    if conn.halted do
      conn
    else
      case Logs.put_retention_days(params["days"]) do
        {:ok, days} -> json(conn, %{days: days})
        {:error, message} -> conn |> put_status(400) |> json(%{error: message})
      end
    end
  end

  def stats(conn, params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_stats(conn, params)
  end

  def delete_logs(conn, params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: do_delete_logs(conn, params)
  end

  # ── Superuser-guarded implementations ──

  # WHERE fragments for the list query: collection first, then status, so the
  # `$N` placeholders stay in order.
  defp list_conditions(params) do
    {conds, args} =
      case params["collection"] do
        collection when is_binary(collection) and collection != "" ->
          {["collection = $1"], [collection]}

        _ ->
          {[], []}
      end

    case status_condition(params["status"]) do
      nil ->
        {conds, args}

      {:eq, code} ->
        {conds ++ ["status = $#{length(args) + 1}"], args ++ [code]}

      {:range, lo, hi} ->
        {conds ++ ["status >= $#{length(args) + 1} AND status < $#{length(args) + 2}"],
         args ++ [lo, hi]}
    end
  end

  # Accepts a status class (`4xx`) or an exact code (`500`); anything else
  # means "all".
  defp status_condition(nil), do: nil
  defp status_condition(""), do: nil

  defp status_condition(status) when is_binary(status) do
    cond do
      Regex.match?(~r/^[1-5]xx$/i, status) ->
        n = status |> String.first() |> String.to_integer()
        {:range, n * 100, (n + 1) * 100}

      Regex.match?(~r/^\d{3}$/, status) ->
        {:eq, String.to_integer(status)}

      true ->
        nil
    end
  end

  defp status_condition(_), do: nil

  defp do_list(conn, params) do
    page = max(1, (params["page"] || "1") |> String.to_integer())
    per_page = min(200, max(1, (params["perPage"] || "50") |> String.to_integer()))
    offset = (page - 1) * per_page

    {conditions, filter_params} = list_conditions(params)
    where_clause = if conditions == [], do: "", else: "WHERE " <> Enum.join(conditions, " AND ")

    total =
      case Ecto.Adapters.SQL.query(
             Repo,
             "SELECT COUNT(*) FROM _request_logs #{where_clause}",
             filter_params
           ) do
        {:ok, %{rows: [[count]]}} -> count
        _ -> 0
      end

    order_dir = if params["order"] == "asc", do: "ASC", else: "DESC"

    select_where =
      "#{where_clause} ORDER BY created_at #{order_dir} " <>
        "LIMIT $#{length(filter_params) + 1} OFFSET $#{length(filter_params) + 2}"

    items =
      case Ecto.Adapters.SQL.query(
             Repo,
             "SELECT id, method, path, status, duration, ip, user_agent, referer, collection, error, body, created_at FROM _request_logs #{select_where}",
             filter_params ++ [per_page, offset]
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
              body,
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
              "body" => body,
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
           "SELECT id, method, path, status, duration, ip, user_agent, referer, collection, error, body, created_at FROM _request_logs WHERE id = $1",
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
             body,
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
          "body" => body,
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

  # ── Stats (multi-metric time series for the chart) ──

  # range → {window interval, bucket size}. Bucket sizes are fixed strings
  # (no user input reaches the SQL), aligned on UTC boundaries via date_bin.
  @ranges %{
    "24h" => {"24 hours", "1 hour"},
    "7d" => {"7 days", "6 hours"},
    "30d" => {"30 days", "1 day"}
  }

  defp do_stats(conn, params) do
    range = if Map.has_key?(@ranges, params["range"]), do: params["range"], else: "24h"
    {interval, bucket} = Map.fetch!(@ranges, range)

    total =
      case Ecto.Adapters.SQL.query(
             Repo,
             "SELECT COUNT(*) FROM _request_logs",
             []
           ) do
        {:ok, %{rows: [[count]]}} -> count
        _ -> 0
      end

    # Per-bucket multi-metric series over the selected window. Always returns
    # a zero-filled series (requests / errors / avg duration per bucket) so the
    # chart renders a complete window even with little or no data.
    series =
      case Ecto.Adapters.SQL.query(
             Repo,
             """
             WITH buckets AS (
               SELECT date_bin(interval '#{bucket}', gs, timestamptz '2000-01-01') AS t
               FROM generate_series(
                 date_bin(interval '#{bucket}', now(), timestamptz '2000-01-01') - interval '#{interval}',
                 now(),
                 interval '#{bucket}'
               ) AS gs
             ),
             agg AS (
               SELECT date_bin(interval '#{bucket}', created_at, timestamptz '2000-01-01') AS t,
                      COUNT(*) AS total,
                      COUNT(*) FILTER (WHERE status >= 400) AS errors,
                      AVG(duration) AS avg_duration
               FROM _request_logs
               WHERE created_at > now() - interval '#{interval}'
               GROUP BY t
             )
             SELECT b.t, COALESCE(a.total, 0), COALESCE(a.errors, 0), COALESCE(a.avg_duration, 0)
             FROM buckets b
             LEFT JOIN agg a ON a.t = b.t
             ORDER BY b.t
             """,
             []
           ) do
        {:ok, result} ->
          Enum.map(result.rows, fn [t, total, errors, avg_duration] ->
            %{
              date: maybe_iso8601(t),
              total: total,
              errors: errors,
              avg_duration: coerce_avg(avg_duration)
            }
          end)

        {:error, _} ->
          []
      end

    json(conn, %{
      total: total,
      range: range,
      series: series,
      summary: %{
        requests: Enum.reduce(series, 0, fn s, acc -> acc + s.total end),
        errors: Enum.reduce(series, 0, fn s, acc -> acc + s.errors end),
        avg_duration:
          coerce_avg(
            case Ecto.Adapters.SQL.query(
                   Repo,
                   "SELECT COALESCE(AVG(duration), 0) FROM _request_logs WHERE created_at > now() - interval '#{interval}'",
                   []
                 ) do
              {:ok, %{rows: [[avg]]}} -> avg
              _ -> 0
            end
          )
      }
    })
  end

  # AVG() returns Postgres numeric (Postgrex → Decimal), which the JSON
  # encoder stringifies. Normalize to a plain float so the API contract is
  # a number.
  defp coerce_avg(%Decimal{} = d), do: Decimal.to_float(d)
  defp coerce_avg(v) when is_number(v), do: v

  defp coerce_avg(v) when is_binary(v) do
    case Float.parse(v) do
      {f, ""} -> f
      _ -> 0
    end
  end

  defp coerce_avg(_), do: 0

  defp do_delete_logs(conn, params) do
    if params["all"] == "true" do
      Logs.clear_all()
      json(conn, %{ok: true, all: true})
    else
      case delete_days(params) do
        days when is_integer(days) ->
          json(conn, %{ok: true, days: days, deleted: Logs.delete_older_than(days)})

        nil ->
          json(conn, %{ok: true, days: nil, deleted: 0})
      end
    end
  end

  # `?days=N` wins; otherwise the configured retention; otherwise 7 (the old
  # hard-coded default) so the manual cleanup keeps working when auto-clean is
  # off. `?days=0` deletes nothing.
  defp delete_days(params) do
    case params["days"] do
      nil ->
        Logs.retention_days() || 7

      value ->
        case Integer.parse(to_string(value)) do
          {n, _} when n > 0 -> n
          _ -> nil
        end
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
