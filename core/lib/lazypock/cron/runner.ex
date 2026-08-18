defmodule Lazypock.Cron.Runner do
  @moduledoc """
  Executes a single cron job action.

  Supported actions:

    * `"http"` — webhook request via `:httpc` (`method`, `url`, `headers`,
      `body` in the job config). Non-2xx responses are treated as errors.
    * `"sql"` — run a `statement` from the job config through
      `Ecto.Adapters.SQL`.
    * `"hook"` — dispatch the app-level `on_cron` event
      (`Lazypock.Hooks.Cron`), letting user hook modules react to the job.

  Every run is wrapped in a Postgres **transaction-scoped advisory lock**
  (`pg_try_advisory_xact_lock`) keyed by the job id, so when multiple BEAM
  nodes share one database a job is never executed twice concurrently. The
  lock auto-releases on commit/rollback — it can never leak on a pooled
  connection. A run that finds the lock already held returns
  `{:skipped, :locked}`.

  Returns `{:ok, result}` | `{:error, reason}` | `{:skipped, :locked}`.
  The outcome is recorded on the job row by `Lazypock.Cron.record_run!/3`.
  """

  alias Lazypock.Repo

  @default_timeout 30_000

  @doc "Runs the job's action (see moduledoc)."
  @spec run(map(), pos_integer()) ::
          {:ok, map()} | {:error, String.t()} | {:skipped, :locked}
  def run(job, timeout \\ @default_timeout) do
    lock_key = "lazypock_cron:" <> to_string(job["id"])

    Repo.transaction(fn ->
      case Repo.query("SELECT pg_try_advisory_xact_lock(hashtext($1))", [lock_key]) do
        {:ok, %{rows: [[true]]}} -> run_action(job, timeout)
        _ -> {:skipped, :locked}
      end
    end)
    |> unwrap()
  rescue
    e -> {:error, Exception.message(e)}
  end

  # ── Actions ────────────────────────────────────────

  defp run_action(job, timeout) do
    case job["action"] do
      "http" -> run_http(job, timeout)
      "sql" -> run_sql(job, timeout)
      "hook" -> run_hook(job)
      other -> {:error, "Unknown action \"#{other}\""}
    end
  end

  defp run_http(job, timeout) do
    config = job["config"] || %{}
    url = config["url"]
    method = config["method"] || "GET"
    headers = config["headers"] || %{}
    body = config["body"]

    request = build_request(method, url, headers, body)
    http_opts = [timeout: timeout, connect_timeout: timeout]

    case :httpc.request(method_atom(method), request, http_opts, body_format: :binary) do
      {:ok, {{_http_version, status, _reason}, _resp_headers, _body}} when status in 200..299 ->
        {:ok, %{status: status}}

      {:ok, {{_http_version, status, _reason}, _resp_headers, _body}} ->
        {:error, "HTTP #{status}"}

      {:error, reason} ->
        {:error, inspect(reason)}
    end
  end

  defp run_sql(job, timeout) do
    statement = (job["config"] || %{})["statement"]

    cond do
      not is_binary(statement) or String.trim(statement) == "" ->
        {:error, "SQL statement is required"}

      true ->
        case Ecto.Adapters.SQL.query(Repo, statement, [], timeout: timeout) do
          {:ok, %{num_rows: n, columns: nil}} ->
            {:ok, %{rows_affected: n}}

          {:ok, %{columns: columns, rows: rows}} ->
            {:ok, %{rows: length(rows), columns: length(columns)}}

          {:error, err} ->
            {:error, Exception.message(err)}
        end
    end
  end

  defp run_hook(job) do
    Lazypock.Hooks.Cron.trigger(job)
  end

  # ── Helpers ────────────────────────────────────────

  defp unwrap({:ok, result}), do: result
  defp unwrap({:error, %{__exception__: true} = e}), do: {:error, Exception.message(e)}
  defp unwrap({:error, reason}), do: {:error, inspect(reason)}

  defp method_atom(method) do
    method |> String.upcase() |> String.to_existing_atom()
  rescue
    ArgumentError -> :get
  end

  defp build_request(_method, url, headers, body) do
    header_list =
      Enum.map(headers, fn {k, v} -> {to_string(k), to_string(v)} end)

    cond do
      is_binary(body) and body != "" ->
        content_type = content_type(header_list)
        {url, header_list, content_type, body}

      true ->
        {url, header_list}
    end
  end

  defp content_type(header_list) do
    Enum.find_value(header_list, "application/json", fn {k, v} ->
      if String.downcase(k) == "content-type", do: v
    end)
  end
end
