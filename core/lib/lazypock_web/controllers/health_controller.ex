defmodule LazypockWeb.HealthController do
  use LazypockWeb, :controller

  @doc """
  Liveness/readiness probe.

  Reports the version plus the file queues: `deletionQueue` (outbox depth and
  the age of the oldest entry) and `imageQueue` (busy/waiting image slots), so a
  stuck cleanup or an overloaded image queue is visible without a shell.
  """
  def index(conn, _params) do
    version = Application.spec(:lazypock, :vsn) |> to_string()

    json(conn, %{
      "status" => "ok",
      "version" => version,
      "files" => %{
        "deletionQueue" => Lazypock.Files.Reaper.stats(),
        "imageQueue" => Lazypock.Files.Limiter.stats()
      }
    })
  end
end
