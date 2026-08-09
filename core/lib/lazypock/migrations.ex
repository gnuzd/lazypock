defmodule Lazypock.Migrations do
  @moduledoc """
  PocketBase-style migrations for the Lazypock binary.

  Migrations live in a **user-writable directory on disk** (`~/.lazypock/migrations/`
  by default, override with `LAZYPOCK_MIGRATIONS_DIR`), NOT inside the binary.

  On boot:

  1. `sync_builtins!/0` copies the migrations bundled in the release
     (`priv/repo/migrations/*.exs`) into that directory **only if the file
     doesn't already exist** (never overwrites user files or a previous
     version of a migration with the same name).
  2. `run/0` applies all pending migrations from that directory via
     `Ecto.Migrator.run/4` (idempotent — uses `schema_migrations`).

  Users can drop additional `NNNN..._name.exs` files into the migrations
  directory and either restart (auto-migrate runs them) or run
  `lazypock migrate` manually. Set `LAZYPOCK_AUTOMIGRATE=0` to disable
  auto-migrate on boot (then use `lazypock migrate`).

  ## Seeds

  A user-editable seed file lives at `~/.lazypock/seeds.exs` (or
  `LAZYPOCK_SEEDS_FILE`). On first boot the bundled `priv/repo/seeds.exs` is
  copied there; after that it's never overwritten. Seeds run once after
  migrations (tracked in `_seeds_run`), or manually via `lazypock seed`.
  """

  require Logger

  alias Lazypock.Repo

  @builtin_migrations "priv/repo/migrations"
  @builtin_seeds "priv/repo/seeds.exs"
  @seeds_run_table "_seeds_run"

  @doc "Returns the user migrations directory, creating it if needed."
  @spec dir() :: String.t()
  def dir do
    base =
      System.get_env("LAZYPOCK_MIGRATIONS_DIR") ||
        Path.join(user_base(), "migrations")

    File.mkdir_p!(base)
    base
  end

  @doc """
  Copies built-in migration files from the release into the user migrations
  dir, without overwriting existing files. Returns the list of files copied.
  """
  @spec sync_builtins!() :: [String.t()]
  def sync_builtins! do
    src = Application.app_dir(:lazypock, @builtin_migrations)
    dest = dir()

    if File.dir?(src) do
      src
      |> File.ls!()
      |> Enum.filter(&String.ends_with?(&1, ".exs"))
      |> Enum.sort()
      |> Enum.flat_map(fn name ->
        target = Path.join(dest, name)

        if File.exists?(target) do
          []
        else
          File.cp!(Path.join(src, name), target)
          [target]
        end
      end)
    else
      []
    end
  end

  @doc """
  Runs all pending migrations from the user migrations dir (idempotent).

  Returns `:ok` or `{:error, reason}` — never raises.
  """
  @spec run() :: :ok | {:error, term()}
  def run do
    migrations_dir = dir()
    sync_builtins!()

    if Code.ensure_loaded?(Ecto.Migrator) and File.dir?(migrations_dir) do
      case Ecto.Migrator.with_repo(Repo, fn repo ->
             Ecto.Migrator.run(repo, migrations_dir, :up, all: true)
           end) do
        {:ok, _, _} ->
          :ok

        {:error, reason} ->
          Logger.error("Auto-migration failed: #{inspect(reason)}")
          {:error, reason}
      end
    else
      :ok
    end
  end

  @doc "Prints migration status (applied/pending) for the user migrations dir."
  @spec status() :: :ok
  def status do
    migrations_dir = dir()
    sync_builtins!()

    if Code.ensure_loaded?(Ecto.Migrator) and File.dir?(migrations_dir) do
      Ecto.Migrator.with_repo(Repo, fn repo ->
        repo
        |> Ecto.Migrator.migrations(migrations_dir)
        |> Enum.each(fn {state, version, name} ->
          IO.puts("#{state |> to_string() |> String.pad_trailing(7)} #{version} #{name}")
        end)
      end)
    end

    :ok
  end

  @doc """
  Runs the seed file (user dir or bundled) once, after migrations.

  Idempotent: records the seed run in the `#{@seeds_run_table}` table so it
  only runs once per database. Use `lazypock seed --force` to re-run.
  """
  @spec seed(keyword()) :: :ok | {:error, term()}
  def seed(opts \\ []) do
    force? = Keyword.get(opts, :force, false)
    seeds_file = seeds_file()

    cond do
      not File.exists?(seeds_file) ->
        Logger.warning("No seed file at #{seeds_file} — nothing to seed.")
        :ok

      not force? and seed_already_run?() ->
        Logger.info("Seeds already run (see #{@seeds_run_table}) — skipping.")
        :ok

      true ->
        case Ecto.Migrator.with_repo(Repo, fn _repo -> run_seed_file!(seeds_file) end) do
          {:ok, result, _} ->
            result

          {:error, reason} ->
            Logger.error("Seed failed: #{inspect(reason)}")
            {:error, reason}
        end
    end
  end

  @doc "Returns the seed file path (user dir or bundled, copied on first boot)."
  @spec seeds_file() :: String.t()
  def seeds_file do
    if path = System.get_env("LAZYPOCK_SEEDS_FILE") do
      path
    else
      dest = Path.join(user_base(), "seeds.exs")

      if not File.exists?(dest) do
        src = Application.app_dir(:lazypock, @builtin_seeds)
        if File.exists?(src), do: File.cp!(src, dest)
      end

      dest
    end
  end

  defp seed_already_run? do
    try do
      Ecto.Migrator.with_repo(Repo, fn _repo ->
        case Repo.query("SELECT 1 FROM #{@seeds_run_table} LIMIT 1") do
          {:ok, %{num_rows: n}} when n > 0 -> true
          _ -> false
        end
      end)
      |> case do
        {:ok, result, _} -> result
        _ -> false
      end
    rescue
      _ -> false
    end
  end

  defp run_seed_file!(seeds_file) do
    result =
      Code.eval_file(seeds_file)

    # Record the run (idempotency marker)
    Ecto.Adapters.SQL.query!(
      Repo,
      "CREATE TABLE IF NOT EXISTS #{@seeds_run_table} (id serial PRIMARY KEY, ran_at TIMESTAMPTZ NOT NULL DEFAULT now(), file TEXT)",
      []
    )

    Ecto.Adapters.SQL.query!(
      Repo,
      "INSERT INTO #{@seeds_run_table} (file) VALUES ($1)",
      [seeds_file]
    )

    Logger.info("Seeds run from #{seeds_file}")
    result
  rescue
    e ->
      Logger.error("Seed failed: #{Exception.message(e)}")
      {:error, e}
  end

  defp user_base do
    System.get_env("LAZYPOCK_DATA_DIR") || Path.join(System.user_home!(), ".lazypock")
  end
end
