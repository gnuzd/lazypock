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
        # Run Ecto migrations (idempotent — only applies pending ones)
        migrate!()

        # Boot-time setup: create _superusers table + auto-create from env
        Lazypock.Auth.Setup.ensure_superusers_table!()
        Lazypock.Auth.Setup.create_from_env!()

        # Create _files table for file storage
        Lazypock.Files.Store.ensure_files_table!()

        # Create ETS rate limiter table (owned by the Application process)
        Lazypock.Auth.RateLimiter.ensure_table()

        # Discover + register user hooks (priv/hooks/*.ex)
        Lazypock.Hooks.Registry.discover!()

        # Fire onBootstrap (PocketBase parity)
        Lazypock.Hooks.App.trigger_bootstrap()

        # Keep the BEAM alive — Burrito's Go wrapper exits the process when the
        # boot script returns. In test, ExUnit manages the lifecycle itself.
        # Code.ensure_loaded? avoids calling Mix.env() which crashes in releases.
        unless Code.ensure_loaded?(ExUnit) do
          Process.sleep(:infinity)
        end

        {:ok, pid}

      error ->
        error
    end
  end

  # Runs all pending Ecto migrations. Uses `Ecto.Migrator` which is available
  # at runtime (not a compile-time dependency). Works in both Mix and release mode.
  defp migrate! do
    migrations_path = Application.app_dir(:lazypock, "priv/repo/migrations")

    if Code.ensure_loaded?(Ecto.Migrator) and File.dir?(migrations_path) do
      {:ok, _, _} =
        Ecto.Migrator.with_repo(Lazypock.Repo, fn repo ->
          Ecto.Migrator.run(repo, migrations_path, :up, all: true)
        end)
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
