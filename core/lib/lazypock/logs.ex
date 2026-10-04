defmodule Lazypock.Logs do
  @moduledoc """
  Request-log housekeeping: the retention setting and the cleanup queries.

  Retention lives at `_settings.data["logs"]["retention_days"]`:

    * `nil` / absent / `0` — auto-clean **off**: entries stay until cleared
      manually.
    * positive integer — entries older than that many days are deleted.

  `Lazypock.Logs.Cleaner` applies the setting on a timer; the Studio exposes the
  same value through a dropdown plus a manual "Clean now" (`DELETE /api/logs`).
  """

  alias Lazypock.Repo
  alias Lazypock.Settings

  @key "logs"

  @doc "Configured retention in days, or `nil` when auto-clean is off."
  @spec retention_days() :: pos_integer() | nil
  def retention_days do
    case Settings.get(@key, %{}) do
      %{"retention_days" => days} when is_integer(days) and days > 0 -> days
      _ -> nil
    end
  rescue
    _ -> nil
  end

  @doc """
  Persist the retention. `nil` / `0` disables auto-clean.

  Returns `{:ok, days | nil}` or `{:error, message}`.
  """
  @spec put_retention_days(term()) :: {:ok, pos_integer() | nil} | {:error, String.t()}
  def put_retention_days(value) do
    case normalize_days(value) do
      {:ok, days} ->
        logs =
          case Settings.get(@key, %{}) do
            map when is_map(map) -> map
            _ -> %{}
          end

        Settings.put(Map.put(Settings.get(), @key, Map.put(logs, "retention_days", days)))
        {:ok, days}

      {:error, _} = error ->
        error
    end
  end

  @doc "Deletes entries older than `days` and returns the deleted count."
  @spec delete_older_than(pos_integer()) :: non_neg_integer()
  def delete_older_than(days) when is_integer(days) and days > 0 do
    {:ok, %{num_rows: count}} =
      Ecto.Adapters.SQL.query(
        Repo,
        "DELETE FROM _request_logs WHERE created_at < now() - make_interval(days => $1)",
        [days]
      )

    count
  end

  @doc "Deletes entries older than the configured retention (no-op when off)."
  @spec clean_old() :: non_neg_integer()
  def clean_old do
    case retention_days() do
      nil -> 0
      days -> delete_older_than(days)
    end
  end

  @doc "Removes every entry."
  @spec clear_all() :: :ok
  def clear_all do
    Ecto.Adapters.SQL.query!(Repo, "TRUNCATE _request_logs", [])
    :ok
  end

  # ── Private ──────────────────────────────────────────

  defp normalize_days(nil), do: {:ok, nil}
  defp normalize_days(0), do: {:ok, nil}
  defp normalize_days(""), do: {:ok, nil}
  defp normalize_days("0"), do: {:ok, nil}
  defp normalize_days(days) when is_integer(days) and days > 0, do: {:ok, days}

  defp normalize_days(days) when is_binary(days) do
    case Integer.parse(String.trim(days)) do
      {n, ""} when n > 0 -> {:ok, n}
      {0, ""} -> {:ok, nil}
      _ -> {:error, "retention_days must be a positive number of days, or 0 to disable"}
    end
  end

  defp normalize_days(_),
    do: {:error, "retention_days must be a positive number of days, or 0 to disable"}
end
