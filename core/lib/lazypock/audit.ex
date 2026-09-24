defmodule Lazypock.Audit do
  @moduledoc """
  Structured audit log for privileged operations.

  Emits one greppable `Logger.info` line per action, e.g.

      [audit] action=import actor=admin@example.com imported=2 errors=0 rolled_back=false

  Covers the operations that can read or rewrite the whole database
  (backup/export, import/restore, rollback), so an operator can see *who* did
  *what* independent of the generic request logger (which does not record the
  acting identity).
  """
  require Logger

  @doc """
  Records one audit line. `actor` is normally the superuser's email;
  `metadata` is a keyword list or map of extra fields.
  """
  @spec record(String.t(), String.t() | nil, keyword() | map()) :: :ok
  def record(action, actor, metadata \\ []) do
    details =
      metadata
      |> Enum.map(fn {key, value} -> "#{key}=#{format(value)}" end)
      |> Enum.join(" ")

    suffix = if details == "", do: "", else: " " <> details
    Logger.info("[audit] action=#{action} actor=#{actor || "unknown"}#{suffix}")
    :ok
  end

  defp format(value) when is_binary(value), do: value
  defp format(value), do: inspect(value)
end
