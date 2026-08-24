defmodule Lazypock.Application do
  # See https://elixir.hexdocs.pm/Application.html
  # for more information on OTP Applications
  @moduledoc false

  use Application

  @impl true
  def start(_type, _args) do
    # CLI commands run before the app boots (migrations don't need the full
    # supervision tree, and Ecto.Migrator starts the repo itself).
    case System.argv() do
      ["migrate"] ->
        Lazypock.Migrations.run()
        System.halt(0)

      ["migrations"] ->
        Lazypock.Migrations.status()
        System.halt(0)

      ["seed"] ->
        Lazypock.Migrations.seed(force: true)
        System.halt(0)

      ["seed", "--force"] ->
        Lazypock.Migrations.seed(force: true)
        System.halt(0)

      ["backup"] ->
        backup(default_backup_path())
        System.halt(0)

      ["backup", path] ->
        backup(path)
        System.halt(0)

      ["restore", path] ->
        restore(path)
        System.halt(0)

      _ ->
        start_app()
    end
  end

  defp backup(path) do
    Ecto.Migrator.with_repo(Lazypock.Repo, fn _repo ->
      payload = Lazypock.Backup.export()
      File.mkdir_p!(Path.dirname(path))
      File.write!(path, Jason.encode!(payload, pretty: true))
      IO.puts("Backup written to #{path}")
    end)
  end

  defp restore(path) do
    if not File.exists?(path) do
      IO.puts("Backup file not found: #{path}")
      System.halt(1)
    end

    payload =
      case File.read(path) do
        {:ok, contents} ->
          Jason.decode!(contents)

        {:error, reason} ->
          IO.puts("Failed to read #{path}: #{inspect(reason)}")
          System.halt(1)
      end

    Ecto.Migrator.with_repo(Lazypock.Repo, fn _repo ->
      result = Lazypock.Backup.restore(payload, true)

      Enum.each(result.imported, fn %{name: name, records_imported: count} ->
        IO.puts("  ✓ #{name} (#{count} records)")
      end)

      Enum.each(result.errors, fn %{name: name, error: error} ->
        IO.puts("  ✗ #{name}: #{inspect(error)}")
      end)

      IO.puts("Restored #{length(result.imported)} collections, #{length(result.errors)} errors")
    end)
  end

  defp default_backup_path do
    date = DateTime.utc_now() |> Calendar.strftime("%Y-%m-%d")
    "lazypock-backup-#{date}.json"
  end

  defp start_app do
    # Run Ecto migrations BEFORE starting any children that touch the DB.
    # Lazypock.Collections.Registry does a SELECT on _collections in its
    # init/1, so on a fresh database it crashes if migrations haven't run.
    # Set LAZYPOCK_AUTOMIGRATE=0 to skip (then use `lazypock migrate`).
    if System.get_env("LAZYPOCK_AUTOMIGRATE") != "0" do
      Lazypock.Migrations.run()
    end

    children =
      [
        LazypockWeb.Telemetry,
        Lazypock.Repo,
        {DNSCluster, query: Application.get_env(:lazypock, :dns_cluster_query) || :ignore},
        {Phoenix.PubSub, name: Lazypock.PubSub},
        Lazypock.Collections.Registry
      ] ++
        cron_scheduler_children() ++
        [LazypockWeb.Endpoint]

    opts = [strategy: :one_for_one, name: Lazypock.Supervisor]

    case Supervisor.start_link(children, opts) do
      {:ok, pid} ->
        # Run seeds once (idempotent, tracked in `_seeds_run`) AFTER the
        # supervision tree is up: the bundled seed creates the example "posts"
        # collection via DDL.create_collection, whose post-commit broadcast to
        # Lazypock.PubSub would crash ("unknown registry") if PubSub had not
        # started yet — and the Registry subscribes to schema broadcasts, so it
        # picks up the seeded collection automatically.
        if System.get_env("LAZYPOCK_AUTOSEED") != "0" do
          Lazypock.Migrations.seed()
        end

        # Boot-time setup: create _superusers table + auto-create from env
        Lazypock.Auth.Setup.ensure_superusers_table!()
        Lazypock.Auth.Setup.create_from_env!()

        # Create _files table for file storage
        Lazypock.Files.Store.ensure_files_table!()

        # Create _external_auths table for OAuth2 provider linking
        Lazypock.Auth.OAuth2.ensure_external_auths_table!()
        # Create OAuth2 session store (state → provider/collection/verifier)
        Lazypock.Auth.OAuth2.ensure_session_table!()

        # Create ETS rate limiter table (owned by the Application process)
        Lazypock.Auth.RateLimiter.ensure_table()

        # Load user hooks (PocketBase pb_hooks style — ~/.lazypock/hooks/*.ex)
        Lazypock.Hooks.User.load!()

        # Discover + register built-in hooks (priv/hooks/*.ex)
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

  # The cron scheduler is skipped under test: its DB access from a non-owner
  # process blocks on sandboxed connections and crash-loops the supervisor.
  # Returns a (possibly empty) child list — supervisors flatten nested lists.
  defp cron_scheduler_children do
    if Application.get_env(:lazypock, :start_cron_scheduler, true) do
      [Lazypock.Cron.Scheduler]
    else
      []
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
