defmodule Lazypock.Migrations do
  @moduledoc """
  PocketBase-style migrations for the Lazypock binary.

  Two independent migration sources, both tracked in `schema_migrations`:

  1. **System migrations** — the schema for LazyPock's own internal tables
     (`_collections`, `_fields`, `_request_logs`, ...). They ship **inside**
     the release (`priv/repo/migrations/`) and are applied directly from
     there on boot. They are never written to a user-visible directory.

  2. **User migrations** — live in a user-writable directory on disk
     (`~/.lazypock/migrations/` by default, override with
     `LAZYPOCK_MIGRATIONS_DIR`). Drop new `NNNN..._name.exs` files there and
     either restart (auto-migrate runs them) or run `lazypock migrate`.

  System migrations always run before user migrations, so user code can rely
  on the system tables existing. Both runs are idempotent
  (`schema_migrations`). Set `LAZYPOCK_AUTOMIGRATE=0` to disable auto-migrate
  on boot (then use `lazypock migrate`).

  If a user migration file shares its version with a bundled system
  migration, it is ignored (the version is already applied from the system
  dir) and a warning is logged. This usually happens with stale copies of
  system migrations copied by older releases — those files are safe to delete.

  ## Seeds

  A user-editable seed file lives at `~/.lazypock/seeds.exs` (or
  `LAZYPOCK_SEEDS_FILE`). On first boot the bundled `priv/repo/seeds.exs` is
  copied there; after that it's never overwritten. Seeds run once after
  migrations (tracked in `_seeds_run`), or manually via `lazypock seed`.

  ## Creating collections from migrations

  A raw `create table(:posts)` migration creates a bare Postgres table that
  is **not** a LazyPock collection — it won't show up in the Studio or be
  served through the dynamic `/api/:collection` routes. To create a real
  collection from a migration, use the helpers below (they delegate to the
  DDL engine and register the collection + fields metadata):

      defmodule Lazypock.Repo.Migrations.AddPosts do
        use Ecto.Migration

        def up do
          Lazypock.Migrations.create_collection("posts",
            type: "base",
            fields: [
              %{"name" => "title", "type" => "text", "required" => true},
              %{"name" => "published", "type" => "bool", "options" => %{"defaultValue" => false}}
            ]
          )
        end

        def down do
          Lazypock.Migrations.drop_collection("posts")
        end
      end

  After every run, any public tables that exist but are **not** registered as
  collections are reported as a warning (raw `create table` migrations, or
  tables you manage yourself) — they stay untouched, they just aren't
  served through the collections API/Studio.
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

  @doc "Returns the bundled system migrations directory inside the release."
  @spec system_dir() :: String.t()
  def system_dir do
    Application.app_dir(:lazypock, @builtin_migrations)
  end

  @doc """
  Runs all pending migrations — system (bundled) first, then user —
  idempotently.

  Returns `:ok` or `{:error, reason}` — never raises.
  """
  @spec run() :: :ok | {:error, term()}
  def run do
    warn_on_stale_system_copies!()

    with :ok <- migrate_dir(system_dir(), "system"),
         :ok <- migrate_dir(dir(), "user") do
      warn_on_unregistered_tables!()
      :ok
    end
  end

  @doc "Prints migration status (applied/pending) for system and user dirs."
  @spec status() :: :ok
  def status do
    warn_on_stale_system_copies!()

    system_versions = migration_versions(system_dir())
    user_versions = migration_versions(dir())

    IO.puts("System migrations (bundled in release):")
    print_status(system_dir(), MapSet.difference(user_versions, system_versions))

    IO.puts("")
    IO.puts("User migrations (#{dir()}):")
    print_status(dir(), system_versions)

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

  # ── Collection helpers (callable from user migrations / seeds) ──
  #
  # These delegate to the DDL engine so a migration can create a REAL
  # collection (table + `_collections` + `_fields` metadata) that shows up
  # in the Studio and is served by the dynamic collection API.
  #
  # Broadcasts are no-ops while PubSub isn't running (CLI migrate / boot),
  # so calling these from a migration is safe — the registry reconciles
  # from the DB at startup.

  @doc """
  Creates a collection (table + metadata) from a migration or seed.

  Same contract as `Lazypock.Schema.DDL.create_collection/2` — returns
  `{:ok, collection}` or `{:error, reason}`. Fails if the collection
  already exists.

  ## Options

    * `:type` — `"base"` (default) or `"auth"`
    * `:fields` — list of field definition maps
    * `:rules` / `:options` / `:hooks` / `:indexes` — optional metadata
  """
  @spec create_collection(String.t(), keyword()) ::
          {:ok, Lazypock.Collections.Collection.t()} | {:error, term()}
  def create_collection(name, opts \\ []) when is_binary(name) do
    Lazypock.Schema.DDL.create_collection(name, opts)
  end

  @doc """
  Updates a collection (rename, add/remove fields, rules, options, hooks).

  Same contract as `Lazypock.Schema.DDL.update_collection/2`. Omitting
  `:fields` leaves columns untouched.
  """
  @spec update_collection(String.t(), keyword()) ::
          {:ok, Lazypock.Collections.Collection.t()} | {:error, term()}
  def update_collection(name, opts \\ []) when is_binary(name) do
    Lazypock.Schema.DDL.update_collection(name, opts)
  end

  @doc """
  Adds a single field (column + metadata) to a collection.

  Same contract as `Lazypock.Schema.DDL.add_field/2`.
  """
  @spec add_field(String.t(), map()) :: :ok | {:error, term()}
  def add_field(collection_name, field_def) when is_binary(collection_name) do
    Lazypock.Schema.DDL.add_field(collection_name, field_def)
  end

  @doc """
  Removes a single field (column + metadata) from a collection.

  Same contract as `Lazypock.Schema.DDL.drop_field/2`.
  """
  @spec drop_field(String.t(), String.t()) :: :ok | {:error, term()}
  def drop_field(collection_name, field_name)
      when is_binary(collection_name) and is_binary(field_name) do
    Lazypock.Schema.DDL.drop_field(collection_name, field_name)
  end

  @doc """
  Drops a managed collection (table + metadata). System collections are
  protected. Same contract as `Lazypock.Schema.DDL.drop_collection/1`.
  """
  @spec drop_collection(String.t()) :: :ok | {:error, term()}
  def drop_collection(name) when is_binary(name) do
    Lazypock.Schema.DDL.drop_collection(name)
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

  # ── Internals ──────────────────────────────────────────────────────────────

  # Applies all pending migrations from a single directory (idempotent).
  defp migrate_dir(migration_dir, label) do
    if Code.ensure_loaded?(Ecto.Migrator) and File.dir?(migration_dir) do
      case Ecto.Migrator.with_repo(Repo, fn repo ->
             Ecto.Migrator.run(repo, migration_dir, :up, all: true)
           end) do
        {:ok, _, _} ->
          :ok

        {:error, reason} ->
          Logger.error("#{label} migration failed: #{inspect(reason)}")
          {:error, reason}
      end
    else
      :ok
    end
  end

  # Prints applied/pending for one dir, hiding versions that belong to the
  # other source (they'd otherwise show up as "FILE NOT FOUND").
  defp print_status(migration_dir, excluded_versions) do
    if Code.ensure_loaded?(Ecto.Migrator) and File.dir?(migration_dir) do
      Ecto.Migrator.with_repo(Repo, fn repo ->
        repo
        |> Ecto.Migrator.migrations(migration_dir)
        |> Enum.reject(fn {_state, version, _name} ->
          MapSet.member?(excluded_versions, Integer.to_string(version))
        end)
        |> Enum.each(fn {state, version, name} ->
          IO.puts("  #{state |> to_string() |> String.pad_trailing(7)} #{version} #{name}")
        end)
      end)
    else
      IO.puts("  (no migrations)")
    end
  end

  # Detect user-dir files whose version collides with a bundled system
  # migration (typically stale copies from the old copy-to-user-dir behavior)
  # and warn that they are ignored / safe to delete.
  defp warn_on_stale_system_copies! do
    system_versions = migration_versions(system_dir())
    user_dir = dir()

    case File.ls(user_dir) do
      {:ok, files} ->
        stale =
          files
          |> Enum.filter(&String.ends_with?(&1, ".exs"))
          |> Enum.filter(fn name -> MapSet.member?(system_versions, migration_version(name)) end)

        if stale != [] do
          Logger.warning(
            "User migration file(s) share a version with bundled system migrations " <>
              "and are ignored: #{Enum.join(stale, ", ")}. " <>
              "These are usually copies of the old sync behavior — safe to delete."
          )
        end

      _ ->
        :ok
    end
  end

  defp migration_versions(migration_dir) do
    case File.ls(migration_dir) do
      {:ok, files} ->
        files
        |> Enum.filter(&String.ends_with?(&1, ".exs"))
        |> Enum.map(&migration_version/1)
        |> Enum.reject(&is_nil/1)
        |> MapSet.new()

      _ ->
        MapSet.new()
    end
  end

  # Public tables that exist but are NOT registered in `_collections` — the
  # classic "my migration created the table but the Studio shows nothing"
  # confusion. Raw `create table` migrations do exactly this. We never touch
  # those tables (the user may manage them with raw SQL), we only surface
  # them so the missing registration isn't a silent surprise. Internal tables
  # (`_...`, `schema_migrations`) are expected and skipped.
  defp warn_on_unregistered_tables! do
    # The repo is stopped once each migrate_dir/with_repo returns, so wrap
    # our own read in a fresh with_repo (best-effort, never raises).
    try do
      Ecto.Migrator.with_repo(Repo, fn _repo -> do_warn_on_unregistered_tables() end)
    rescue
      _ -> :ok
    end

    :ok
  end

  defp do_warn_on_unregistered_tables do
    registered =
      case Repo.query("SELECT name FROM _collections", []) do
        {:ok, %{rows: rows}} -> rows |> List.flatten() |> MapSet.new()
        _ -> MapSet.new()
      end

    case Repo.query(
           "SELECT tablename FROM pg_tables WHERE schemaname = 'public'",
           []
         ) do
      {:ok, %{rows: rows}} ->
        tables =
          rows
          |> List.flatten()
          # Internal `_`-prefixed tables + schema_migrations are expected
          # to exist without a `_collections` row — skip them.
          |> Enum.reject(&(String.starts_with?(&1, "_") or &1 == "schema_migrations"))

        unregistered =
          tables
          |> Enum.reject(&MapSet.member?(registered, &1))
          |> Enum.sort()

        if unregistered != [] do
          Logger.warning(
            "Tables exist but are not registered as collections " <>
              "(they won't appear in the Studio / collections API): " <>
              Enum.join(unregistered, ", ") <>
              ". To serve them, create a collection instead of a raw table " <>
              "(see Lazypock.Migrations.create_collection/2), or import a backup " <>
              "that registers them."
          )
        end

      _ ->
        :ok
    end

    :ok
  end

  # Ecto migration version = the numeric prefix before the first `_`
  # (e.g. "20250101000000_create_system_tables.exs" → "20250101000000").
  # Non-migration files (e.g. ".formatter.exs") return nil.
  defp migration_version(filename) do
    case Regex.run(~r/^(\d+)_/, filename) do
      [_, digits] -> digits
      _ -> nil
    end
  end
end
