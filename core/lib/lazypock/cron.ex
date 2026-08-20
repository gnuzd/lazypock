defmodule Lazypock.Cron do
  @moduledoc """
  Persisted cron jobs: validation, next-run computation and CRUD.

  Jobs live in the `_crons` table and are executed by
  `Lazypock.Cron.Scheduler`. All stored timestamps are UTC; each job carries
  an IANA `timezone` (default `"UTC"`) that defines the wall-clock context in
  which its cron `expression` is interpreted. The Studio renders timestamps
  in the browser's local time zone, so admins always see "their" time.

  Expressions accept standard 5-field cron (`min hour day month weekday`) and
  PocketBase-style 6-field with seconds.
  """

  alias Lazypock.Cron.Scheduler
  alias Lazypock.Repo

  @actions ~w(http sql hook)
  @http_methods ~w(GET POST PUT PATCH DELETE HEAD OPTIONS)
  @default_timezone "UTC"

  @type job :: %{
          optional(String.t()) => term()
        }

  # ── List / fetch ───────────────────────────────────

  @doc "Returns all cron jobs, ordered by name."
  @spec list() :: [job()]
  def list do
    case query("SELECT * FROM _crons ORDER BY name", []) do
      {:ok, %{rows: rows}} -> Enum.map(rows, &row_to_map/1)
      _ -> []
    end
  end

  @doc "Returns a single cron job by id, or `nil`."
  @spec get(String.t()) :: job() | nil
  def get(id) when is_binary(id) do
    with {:ok, uuid} <- Ecto.UUID.dump(id) do
      case query("SELECT * FROM _crons WHERE id = $1", [uuid]) do
        {:ok, %{rows: [row]}} -> row_to_map(row)
        _ -> nil
      end
    else
      _ -> nil
    end
  end

  # ── Create / update / delete ───────────────────────

  @doc """
  Creates a cron job from `attrs` (string-keyed). Validates the expression,
  timezone and action config, computes the first `next_run_at` for enabled
  jobs, inserts the row and asks the scheduler to reload.

  Returns `{:ok, job}` or `{:error, message}`.
  """
  @spec create(map()) :: {:ok, job()} | {:error, String.t()}
  def create(attrs) when is_map(attrs) do
    with {:ok, p} <- validate(attrs) do
      next = if p.enabled, do: next_run_at(p.expression, p.timezone), else: nil

      case query(
             """
             INSERT INTO _crons
               (name, expression, timezone, enabled, action, config, next_run_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING *
             """,
             [p.name, p.expression, p.timezone, p.enabled, p.action, p.config, next]
           ) do
        {:ok, %{rows: [row]}} ->
          Scheduler.reload()
          {:ok, row_to_map(row)}

        {:error, %Postgrex.Error{postgres: %{code: :unique_violation}}} ->
          {:error, "A cron job named \"#{p.name}\" already exists"}

        {:error, err} ->
          {:error, Exception.message(err)}
      end
    end
  end

  @doc """
  Updates a cron job. `attrs` may contain any subset of the mutable fields.
  Returns `{:ok, job}`, `{:error, :not_found}` or `{:error, message}`.
  """
  @spec update(String.t(), map()) :: {:ok, job()} | {:error, atom() | String.t()}
  def update(id, attrs) when is_binary(id) and is_map(attrs) do
    case get(id) do
      nil ->
        {:error, :not_found}

      job ->
        merged = merge_attrs(job, attrs)

        with {:ok, p} <- validate(merged) do
          next = if p.enabled, do: next_run_at(p.expression, p.timezone), else: nil

          case query(
                 """
                 UPDATE _crons SET
                   name = $1, expression = $2, timezone = $3, enabled = $4,
                   action = $5, config = $6, next_run_at = $7, updated_at = now()
                 WHERE id = $8
                 RETURNING *
                 """,
                 [p.name, p.expression, p.timezone, p.enabled, p.action, p.config, next, uuid(id)]
               ) do
            {:ok, %{rows: [row]}} ->
              Scheduler.reload()
              {:ok, row_to_map(row)}

            {:error, %Postgrex.Error{postgres: %{code: :unique_violation}}} ->
              {:error, "A cron job named \"#{p.name}\" already exists"}

            {:error, err} ->
              {:error, Exception.message(err)}
          end
        end
    end
  end

  @doc "Deletes a cron job. Returns `:ok` or `:error`."
  @spec delete(String.t()) :: :ok | :error
  def delete(id) when is_binary(id) do
    case query("DELETE FROM _crons WHERE id = $1", [uuid(id)]) do
      {:ok, %{num_rows: 1}} ->
        Scheduler.reload()
        :ok

      _ ->
        :error
    end
  end

  @doc """
  Runs a job immediately (bypassing the schedule), records the outcome and
  reloads the scheduler. Returns `{:ok, updated_job}` or `{:error, :not_found}`.
  """
  @spec run_now(String.t()) :: {:ok, job()} | {:error, :not_found}
  def run_now(id) when is_binary(id) do
    case get(id) do
      nil ->
        {:error, :not_found}

      job ->
        started = System.monotonic_time(:millisecond)
        result = Lazypock.Cron.Runner.run(job)
        duration_ms = System.monotonic_time(:millisecond) - started
        updated = record_run!(job, result, duration_ms)
        Scheduler.reload()
        {:ok, updated}
    end
  end

  @doc """
  Records the outcome of a run on the job row and recomputes `next_run_at`.

  `result` is what `Lazypock.Cron.Runner.run/1` returns:
  `{:ok, map}`, `{:error, reason}` or `{:skipped, :locked}`.
  Returns the updated job map (falls back to the input job when the row is
  gone, e.g. the job was deleted while running).
  """
  @spec record_run!(job(), term(), non_neg_integer()) :: job()
  def record_run!(job, result, duration_ms) do
    {status, error} =
      case result do
        {:ok, _} -> {"ok", nil}
        {:error, reason} -> {"error", humanize_error(reason)}
        {:skipped, :locked} -> {"skipped", "another instance is running this job"}
      end

    next = if job["enabled"], do: next_run_at(job["expression"], job["timezone"]), else: nil

    case query(
           """
           UPDATE _crons SET
             last_run_at = $1, last_status = $2, last_duration_ms = $3,
             last_error = $4, next_run_at = $5, updated_at = now()
           WHERE id = $6
           RETURNING *
           """,
           [DateTime.utc_now(), status, duration_ms, error, next, uuid(job["id"])]
         ) do
      {:ok, %{rows: [row]}} -> row_to_map(row)
      _ -> job
    end
  end

  # ── Validation ─────────────────────────────────────

  @doc """
  Validates and normalizes job attributes. Returns
  `{:ok, %{name:, expression:, timezone:, enabled:, action:, config:}}`
  or `{:error, message}`.
  """
  @spec validate(map()) :: {:ok, map()} | {:error, String.t()}
  def validate(attrs) do
    with {:ok, name} <- validate_name(attrs),
         {:ok, expression} <- validate_expression(attrs["expression"]),
         {:ok, timezone} <- validate_timezone(attrs["timezone"]),
         {:ok, enabled} <- validate_enabled(attrs["enabled"]),
         {:ok, action} <- validate_action(attrs["action"]),
         {:ok, config} <- validate_config(action, attrs["config"]) do
      {:ok,
       %{
         name: name,
         expression: expression,
         timezone: timezone,
         enabled: enabled,
         action: action,
         config: config
       }}
    end
  end

  @doc "Parses a cron expression (5 or 6 fields, or @-macros). Returns `{:ok, %CronExpression{}}` or `{:error, message}`."
  @spec parse_expression(String.t() | nil) ::
          {:ok, Crontab.CronExpression.t()} | {:error, String.t()}
  def parse_expression(nil), do: {:error, "Expression is required"}

  def parse_expression(expr) when is_binary(expr) do
    trimmed = String.trim(expr)

    cond do
      trimmed == "" ->
        {:error, "Expression is required"}

      String.starts_with?(trimmed, "@") ->
        case Crontab.CronExpression.Parser.parse(trimmed) do
          {:ok, %Crontab.CronExpression{reboot: true}} ->
            {:error, "@reboot is not supported"}

          {:ok, %Crontab.CronExpression{} = parsed} ->
            {:ok, parsed}

          {:error, msg} ->
            {:error, msg}
        end

      true ->
        fields = String.split(trimmed)

        case length(fields) do
          5 -> parse_fields(trimmed, false)
          6 -> parse_fields(trimmed, true)
          n -> {:error, "Expected 5 or 6 cron fields (got #{n})"}
        end
    end
  end

  defp parse_fields(expr, extended) do
    case Crontab.CronExpression.Parser.parse(expr, extended) do
      {:ok, %Crontab.CronExpression{} = parsed} -> {:ok, parsed}
      {:error, msg} -> {:error, msg}
    end
  end

  @doc "Returns true when `timezone` is a valid IANA zone name."
  @spec valid_timezone?(String.t()) :: boolean()
  def valid_timezone?(tz) when is_binary(tz) do
    match?({:ok, _}, DateTime.now(tz))
  end

  def valid_timezone?(_), do: false

  @doc "Returns the list of supported action types."
  @spec actions() :: [String.t()]
  def actions, do: @actions

  # ── Next-run computation ───────────────────────────

  @doc """
  Returns the next `count` run times (UTC `DateTime`s) for an expression
  interpreted in `timezone`, starting strictly after `from`.
  Returns `[]` for invalid input.
  """
  @spec next_run_dates(String.t(), String.t(), pos_integer(), DateTime.t()) :: [DateTime.t()]
  def next_run_dates(
        expression,
        timezone \\ @default_timezone,
        count \\ 5,
        from \\ DateTime.utc_now()
      ) do
    with {:ok, expr} <- parse_expression(expression),
         {:ok, zoned} <- DateTime.shift_zone(from, timezone) do
      expr
      |> Crontab.Scheduler.get_next_run_dates(zoned)
      # take(count + 1) then filter: the stream is strictly increasing, so the
      # only candidate that can equal `from` (inclusive boundary match) is the
      # first element. We avoid Enum.drop_while — it hangs on lazy streams.
      |> Enum.take(count + 1)
      |> Enum.filter(&(DateTime.compare(&1, zoned) == :gt))
      |> Enum.take(count)
      |> Enum.map(&DateTime.shift_zone!(&1, "UTC"))
    else
      _ -> []
    end
  end

  @doc "Returns the next run time (UTC `DateTime`) for an expression in a timezone, or `nil`."
  @spec next_run_at(String.t(), String.t(), DateTime.t()) :: DateTime.t() | nil
  def next_run_at(expression, timezone \\ @default_timezone, from \\ DateTime.utc_now()) do
    next_run_dates(expression, timezone, 1, from) |> List.first()
  end

  # ── Private ────────────────────────────────────────

  defp validate_name(attrs) do
    name = attrs["name"]

    cond do
      not is_binary(name) or String.trim(name) == "" ->
        {:error, "Name is required"}

      String.length(String.trim(name)) > 200 ->
        {:error, "Name must be 200 characters or fewer"}

      true ->
        {:ok, String.trim(name)}
    end
  end

  defp validate_expression(expr) do
    case parse_expression(expr) do
      {:ok, _parsed} -> {:ok, String.trim(expr)}
      {:error, msg} -> {:error, msg}
    end
  end

  defp validate_timezone(tz) do
    cond do
      is_nil(tz) ->
        {:ok, @default_timezone}

      not is_binary(tz) ->
        {:error, "Timezone must be a string"}

      valid_timezone?(tz) ->
        {:ok, tz}

      true ->
        {:error, "Unknown timezone \"#{tz}\""}
    end
  end

  defp validate_enabled(enabled) do
    case enabled do
      nil -> {:ok, true}
      true -> {:ok, true}
      false -> {:ok, false}
      v when is_binary(v) -> {:ok, v in ["true", "1"]}
      _ -> {:error, "enabled must be a boolean"}
    end
  end

  defp validate_action(action) do
    case action do
      nil ->
        {:ok, "http"}

      a when a in @actions ->
        {:ok, a}

      a when is_binary(a) ->
        {:error, "Unknown action \"#{a}\" (expected #{Enum.join(@actions, ", ")})"}

      _ ->
        {:error, "Action must be a string"}
    end
  end

  defp validate_config(action, config) do
    config = if is_map(config), do: config, else: %{}

    case action do
      "http" -> validate_http_config(config)
      "sql" -> validate_sql_config(config)
      "hook" -> {:ok, config}
    end
  end

  defp validate_http_config(config) do
    with {:ok, url} <- validate_url(config["url"]) do
      method =
        case config["method"] do
          nil -> "GET"
          m when m in @http_methods -> m
          m when is_binary(m) -> String.upcase(m)
          _ -> "GET"
        end

      if method in @http_methods do
        {:ok,
         %{
           "url" => url,
           "method" => method,
           "headers" => normalize_headers(config["headers"]),
           "body" => config["body"]
         }}
      else
        {:error, "Unsupported HTTP method \"#{method}\""}
      end
    end
  end

  defp validate_url(nil), do: {:error, "URL is required for http actions"}

  defp validate_url(url) when is_binary(url) do
    case URI.parse(url) do
      %URI{scheme: scheme, host: host}
      when scheme in ["http", "https"] and is_binary(host) and host != "" ->
        {:ok, url}

      _ ->
        {:error, "URL must be an absolute http(s) URL"}
    end
  end

  defp validate_url(_), do: {:error, "URL must be a string"}

  defp normalize_headers(nil), do: %{}
  defp normalize_headers(%{} = headers), do: headers
  defp normalize_headers(_), do: %{}

  defp validate_sql_config(config) do
    statement = config["statement"]

    cond do
      not is_binary(statement) or String.trim(statement) == "" ->
        {:error, "SQL statement is required for sql actions"}

      true ->
        {:ok, %{"statement" => statement}}
    end
  end

  # Merge incoming attrs over an existing job (used by update).
  defp merge_attrs(job, attrs) do
    Map.merge(
      %{
        "name" => job["name"],
        "expression" => job["expression"],
        "timezone" => job["timezone"],
        "enabled" => job["enabled"],
        "action" => job["action"],
        "config" => job["config"]
      },
      Map.take(attrs, ["name", "expression", "timezone", "enabled", "action", "config"])
    )
  end

  defp humanize_error(reason) when is_binary(reason), do: reason

  defp humanize_error(reason) do
    reason
    |> inspect(limit: 300)
    |> String.slice(0, 1000)
  end

  defp row_to_map(row) do
    keys = [
      "id",
      "name",
      "expression",
      "timezone",
      "enabled",
      "action",
      "config",
      "last_run_at",
      "last_status",
      "last_duration_ms",
      "last_error",
      "next_run_at",
      "created_at",
      "updated_at"
    ]

    keys
    |> Enum.zip(row)
    |> Map.new()
    |> Map.update!("id", &Ecto.UUID.load!/1)
    |> Map.update("enabled", false, &(&1 == true or &1 == "t"))
    |> Map.new(fn {k, v} -> {k, to_utc_datetime(v)} end)
  end

  # Postgrex returns %NaiveDateTime{} for timestamp-without-tz columns
  # (`utc_datetime_usec`); the stored values are UTC by contract, so lift them
  # to proper UTC DateTimes for comparison and JSON encoding.
  defp to_utc_datetime(%NaiveDateTime{} = ndt), do: DateTime.from_naive!(ndt, "Etc/UTC")
  defp to_utc_datetime(v), do: v

  # Dump a UUID string to its 16-byte binary form (nil-safe for invalid ids).
  defp uuid(id) when is_binary(id) do
    case Ecto.UUID.dump(id) do
      {:ok, bin} -> bin
      :error -> id
    end
  end

  defp query(sql, params) do
    Ecto.Adapters.SQL.query(Repo, sql, params)
  end
end
