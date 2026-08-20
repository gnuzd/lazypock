defmodule Lazypock.Hooks.Cron do
  @moduledoc """
  App-level cron hook — fired whenever a cron job with the `"hook"` action
  executes. Lets user hook modules (`priv/hooks/*.ex`) react to scheduled
  jobs, e.g.:

      use Lazypock.Hooks.Hook

      def on_cron(e) do
        Logger.info("cron job \#{e.data.job["name"]} fired")
        Lazypock.Hooks.Event.next(e)
      end

  The event's `data` map carries the whole job under `:job` (same shape the
  admin API returns, including `config`). The job is executed inside the
  runner's transaction (advisory-lock guarded), so hook code may use
  `Lazypock.Repo` directly.
  """

  alias Lazypock.Hooks.Registry

  @doc "Registers an `on_cron` handler."
  def on_cron(fun) when is_function(fun, 1) do
    Registry.register(:on_cron, {:fun, fun}, collections: :any)
    :ok
  end

  @doc "Fires `on_cron` for a job (called by `Lazypock.Cron.Runner`)."
  def trigger(job) do
    Registry.dispatch(:on_cron, %{job: job})
    {:ok, %{event: :on_cron}}
  end
end
