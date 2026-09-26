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

      ["backup" | args] ->
        backup_cli(args)
        System.halt(0)

      ["restore" | args] ->
        restore_cli(args)
        System.halt(0)

      _ ->
        start_app()
    end
  end

  # ── CLI: backup / restore ────────────────────────────────────────────────

  defp backup_cli(args) do
    {opts, argv} = parse_flags(args, %{json: false})
    format = if opts.json, do: :json, else: :archive
    backup(List.first(argv) || default_backup_path(format))
  end

  defp restore_cli(args) do
    {opts, argv} = parse_flags(args, %{confirmed: false, delete_missing: true})

    case List.first(argv) do
      nil ->
        IO.puts(:stderr, "Usage: lazypock restore <file> [--no-undo-checkpoint] [--keep-missing]")
        System.halt(1)

      path ->
        restore(path, opts)
    end
  end

  defp parse_flags(args, defaults) do
    {opts, argv} =
      Enum.reduce(args, {defaults, []}, fn
        "--json", {opts, acc} -> {Map.put(opts, :json, true), acc}
        "--no-undo-checkpoint", {opts, acc} -> {Map.put(opts, :confirmed, true), acc}
        "--keep-missing", {opts, acc} -> {Map.put(opts, :delete_missing, false), acc}
        arg, {opts, acc} -> {opts, [arg | acc]}
      end)

    {opts, Enum.reverse(argv)}
  end

  # Archives are the default; a `.json` path keeps the legacy single-document
  # behavior for small databases and existing scripts.
  defp backup(path) do
    Ecto.Migrator.with_repo(Lazypock.Repo, fn _repo ->
      if String.ends_with?(path, ".json") do
        payload = Lazypock.Backup.export()
        File.mkdir_p!(Path.dirname(path))
        File.write!(path, Jason.encode!(payload, pretty: true))

        IO.puts(
          "Backup written to #{path} (legacy JSON — the whole database is held in " <>
            "memory, prefer an .zip archive for large databases)"
        )
      else
        File.mkdir_p!(Path.dirname(Path.expand(path)))

        case Lazypock.Backup.export_stream(dest: path) do
          {:ok, %{bytes: bytes, collections: collections, records: records}} ->
            IO.puts(
              "Backup written to #{path} (#{collections} collections, " <>
                "#{records} records, #{format_bytes(bytes)})"
            )

          {:error, reason} ->
            IO.puts(:stderr, "Backup failed: #{inspect(reason)}")
            System.halt(1)
        end
      end
    end)
  end

  defp restore(path, opts) do
    if not File.exists?(path) do
      IO.puts(:stderr, "Backup file not found: #{path}")
      System.halt(1)
    end

    delete_missing = Map.get(opts, :delete_missing, true)

    Ecto.Migrator.with_repo(Lazypock.Repo, fn _repo ->
      preflight = Lazypock.Backup.preflight()

      # Above the threshold there is no automatic undo checkpoint, so require an
      # explicit opt-in rather than silently losing rollback.
      if not preflight.undo_available and not Map.get(opts, :confirmed, false) do
        IO.puts(:stderr, """

        WARNING: this database is #{preflight.db_size_mb} MB, above the \
        #{preflight.threshold_mb} MB automatic-undo threshold.
        No undo checkpoint will be taken, so there is no rollback if this import goes wrong.
        #{neon_note(preflight)}Re-run with --no-undo-checkpoint to proceed anyway.
        """)

        System.halt(2)
      end

      if delete_missing do
        IO.puts(
          "Note: collections absent from #{path} will be dropped " <>
            "(pass --keep-missing to keep them)."
        )
      end

      result =
        if archive?(path) do
          Lazypock.Backup.restore_archive(path, delete_missing, atomic: :batch, snapshot: true)
        else
          Lazypock.Backup.restore(read_json!(path), delete_missing,
            atomic: :batch,
            snapshot: true
          )
        end

      Enum.each(result.imported, fn %{name: name, records_imported: count} ->
        IO.puts("  ✓ #{name} (#{count} records)")
      end)

      Enum.each(result.errors, fn %{name: name, error: error} ->
        IO.puts("  ✗ #{name}: #{inspect(error)}")
      end)

      IO.puts("Restored #{length(result.imported)} collections, #{length(result.errors)} errors")

      if Map.get(result, :rolled_back, false) do
        IO.puts(:stderr, "All changes were rolled back.")
      end
    end)
  end

  defp read_json!(path) do
    case File.read(path) do
      {:ok, contents} ->
        Jason.decode!(contents)

      {:error, reason} ->
        IO.puts(:stderr, "Failed to read #{path}: #{inspect(reason)}")
        System.halt(1)
    end
  end

  # Zip magic rather than the extension, so a renamed archive still works.
  defp archive?(path) do
    case File.open(path, [:read, :binary], fn io -> IO.binread(io, 2) end) do
      {:ok, "PK"} -> true
      _ -> String.ends_with?(path, ".zip")
    end
  end

  defp neon_note(%{neon_hosted: true}) do
    "This looks like a Neon-hosted database — a branch or Neon's point-in-time restore can " <>
      "serve as an alternative safety net (https://neon.tech/docs/introduction/branching).\n"
  end

  defp neon_note(_), do: ""

  defp format_bytes(bytes) when bytes >= 1_073_741_824 do
    "#{Float.round(bytes / 1_073_741_824, 1)} GB"
  end

  defp format_bytes(bytes) when bytes >= 1_048_576 do
    "#{Float.round(bytes / 1_048_576, 1)} MB"
  end

  defp format_bytes(bytes) when bytes >= 1_024, do: "#{Float.round(bytes / 1_024, 1)} KB"
  defp format_bytes(bytes), do: "#{bytes} B"

  defp default_backup_path(format) do
    date = DateTime.utc_now() |> Calendar.strftime("%Y-%m-%d")
    ext = if format == :json, do: "json", else: "zip"
    "lazypock-backup-#{date}.#{ext}"
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
