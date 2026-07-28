defmodule Lazypock.Application do
  # See https://elixir.hexdocs.pm/Application.html
  # for more information on OTP Applications
  @moduledoc false

  use Application

  @impl true
  def start(_type, _args) do
    children = [
      LazypockWeb.Telemetry,
      Lazypock.Repo,
      {DNSCluster, query: Application.get_env(:lazypock, :dns_cluster_query) || :ignore},
      {Phoenix.PubSub, name: Lazypock.PubSub},
      Lazypock.Collections.Registry,
      LazypockWeb.Endpoint
    ]

    opts = [strategy: :one_for_one, name: Lazypock.Supervisor]

    case Supervisor.start_link(children, opts) do
      {:ok, pid} ->
        # Boot-time setup: create _superusers table + auto-create from env
        Lazypock.Auth.Setup.ensure_superusers_table!()
        Lazypock.Auth.Setup.create_from_env!()

        # Create _files table for file storage
        Lazypock.Files.Store.ensure_files_table!()

        # Keep the BEAM alive — Burrito's Go wrapper exits the process when the
        # boot script returns. In test, ExUnit manages the lifecycle itself.
        unless Mix.env() == :test do
          Process.sleep(:infinity)
        end

        {:ok, pid}

      error ->
        error
    end
  end

  # Tell Phoenix to update the endpoint configuration
  # whenever the application is updated.
  @impl true
  def config_change(changed, _new, removed) do
    LazypockWeb.Endpoint.config_change(changed, removed)
    :ok
  end
end
