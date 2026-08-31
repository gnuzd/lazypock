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

  Two ways to create a collection from a migration:

  1. **Explicit** — use the helpers below (they delegate to the DDL engine
     and register the collection + fields metadata). This is the recommended
     path when you want full control (rules, indexes, auth type, ...):

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

     **Relation fields fail fast.** A `relation` field must name its target
     collection — either `options.collection` (the target's name) or a
     `collectionId` (UUID or name) — and that collection must already exist
     in `_collections`. If it doesn't (typo, missing `create_collection` for
     the target, out-of-order creation), the helper returns `{:error, msg}`
     and the migration fails loudly instead of silently persisting a broken
     relation. Create the target collection before the field that references
     it.

  2. **Automatic** — a raw `create table(:posts)` migration "just works".
     After every migration run, any public table that exists but is **not**
     registered in `_collections` is registered as a `base` collection with
     its columns inferred from the Postgres schema (so it shows up in the
     Studio and is served through `/api/:collection`). Foreign-key columns
     (`references(:other)`) are inferred as `relation` fields pointing at
     the referenced table, so the Studio shows a relation dropdown and
     `expand` works on the API. Tables that use Ecto's `timestamps()` get
     their `inserted_at` renamed to LazyPock's `created_at` convention, and
     a missing `updated_at` is added, so create/update/delete work out of
     the box.

     Internal `_`-prefixed tables and `schema_migrations` are never
     touched. Tables created at runtime by the app (request logs, settings,
     files, ...) are internal tables and are skipped the same way.

  Note: registration happens after migrations complete. When migrations run
  at boot (the default), the collections registry starts afterwards and
  picks everything up. When you run `lazypock migrate` against an **already
  running** server, the new collections are written to the DB but the
  running server's registry only refreshes on restart.
  """

  require Logger

  alias Lazypock.Repo
  alias Lazypock.Schema.TypeMapper

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
      register_unregistered_tables!()
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
    with :ok <- validate_relation_targets(Keyword.get(opts, :fields, [])) do
      Lazypock.Schema.DDL.create_collection(name, opts)
    end
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
    with :ok <- validate_relation_targets([field_def]) do
      Lazypock.Schema.DDL.add_field(collection_name, field_def)
    end
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

  # Fail-fast for migration authoring: a relation field must point at a
  # collection that exists in `_collections`. The DDL engine itself stays
  # lenient (the Studio, importer and backup restore tolerate dangling
  # targets), but a migration that creates a broken relation — a typo'd
  # `options.collection` name, an unresolvable `collectionId`, or no target
  # at all — should fail loudly at boot instead of silently persisting a
  # field the Studio dropdown and API `expand` can never resolve.
  defp validate_relation_targets(fields) do
    Enum.reduce_while(fields, :ok, fn
      %{"type" => "relation"} = field, :ok ->
        case Lazypock.Schema.DDL.resolve_relation_target(field) do
          {:ok, _collection} -> {:cont, :ok}
          {:error, msg} -> {:halt, {:error, msg}}
        end

      _field, :ok ->
        {:cont, :ok}
    end)
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
  # confusion. Raw `create table` migrations do exactly this. Instead of
  # leaving those tables invisible (and only warning), we register them as
  # base collections so they show up in the Studio and are served through the
  # dynamic `/api/:collection` routes.
  #
  # Internal tables (`_...`, `schema_migrations`) are expected to exist
  # without a `_collections` row and are always skipped.
  defp register_unregistered_tables! do
    # The repo is stopped once each migrate_dir/with_repo returns, so wrap
    # our own read in a fresh with_repo (best-effort, never raises).
    try do
      Ecto.Migrator.with_repo(Repo, fn _repo -> register_unregistered_tables() end)
    rescue
      _ -> :ok
    end

    :ok
  end

  @doc """
  Registers any public table that exists but is not yet in `_collections` as
  a base collection (columns inferred from the Postgres schema).

  Runs against the currently-started repo (no restart, safe to call from a
  running server / tests). Returns `:ok`; per-table failures are logged as
  warnings, never raised. Internal `_`-prefixed tables and
  `schema_migrations` are skipped.
  """
  @spec register_unregistered_tables() :: :ok
  def register_unregistered_tables do
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

        tables
        |> Enum.reject(&MapSet.member?(registered, &1))
        |> Enum.sort()
        |> Enum.each(&register_table!(&1))

      _ ->
        :ok
    end

    :ok
  end

  # Registers one existing physical table as a base collection (metadata
  # only — the table itself is never recreated). Reconciles the LazyPock
  # shape first so raw Ecto `timestamps()` tables (inserted_at/updated_at)
  # become fully usable: `inserted_at` → `created_at`, missing `updated_at`
  # added. Foreign-key columns are inferred as `relation` fields pointing at
  # the referenced table. Best-effort: failures are logged, never raised.
  defp register_table!(table_name) do
    Repo.transaction(fn ->
      reconcile_timestamps!(table_name)
      columns = fetch_columns!(table_name)
      foreign_keys = fetch_foreign_keys!(table_name)
      fields = infer_fields(columns, foreign_keys)

      collection =
        %Lazypock.Collections.Collection{}
        |> Lazypock.Collections.Collection.changeset(%{
          name: table_name,
          type: "base",
          schema: Enum.map(fields, &schema_entry/1),
          rules: default_base_rules(),
          options: %{"indexes" => []},
          hooks: %{},
          managed: true
        })
        |> Repo.insert!()

      fields
      |> Enum.with_index(1)
      |> Enum.each(fn {field, order} ->
        %Lazypock.Collections.Field{}
        |> Lazypock.Collections.Field.changeset(%{
          collection_id: collection.id,
          name: field["name"],
          type: field["type"],
          required: field["required"],
          unique: field["unique"],
          default_value: field["default"],
          options: field["options"],
          indexed: field["indexed"],
          sort_order: order
        })
        |> Repo.insert!()
      end)

      Logger.info(
        "Registered existing table as collection: #{table_name} " <>
          "(#{length(fields)} fields, type: base)"
      )

      :ok
    end)
  rescue
    e ->
      Logger.warning(
        "Failed to register table #{table_name} as a collection: #{Exception.message(e)}"
      )
  end

  # Ecto `timestamps()` creates `inserted_at`/`updated_at`; LazyPock uses
  # `created_at`/`updated_at`. Rename so inserts/updates work unchanged.
  # Adding missing `updated_at` keeps updates working on half-shaped tables.
  defp reconcile_timestamps!(table_name) do
    columns = fetch_columns!(table_name)
    names = MapSet.new(columns, & &1["column_name"])

    if "inserted_at" in names and "created_at" not in names do
      Ecto.Adapters.SQL.query!(
        Repo,
        "ALTER TABLE #{TypeMapper.quote_ident(table_name)} RENAME COLUMN inserted_at TO created_at",
        []
      )
    end

    if "updated_at" not in names do
      Ecto.Adapters.SQL.query!(
        Repo,
        "ALTER TABLE #{TypeMapper.quote_ident(table_name)} " <>
          "ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT now()",
        []
      )
    end
  end

  defp fetch_columns!(table_name) do
    {:ok, %{rows: rows, columns: cols}} =
      Repo.query(
        """
        SELECT column_name, data_type, udt_name, is_nullable, column_default
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = $1
        ORDER BY ordinal_position
        """,
        [table_name]
      )

    rows
    |> Enum.map(fn row ->
      cols |> Enum.zip(row) |> Map.new()
    end)
  end

  # Maps each foreign-key column of the table to the referenced table name
  # (`column_name => referenced_table`). Lets raw `create table` migrations
  # with `references(...)` columns come through as real `relation` fields
  # (options.collection = target) instead of plain text, so the Studio shows
  # the relation dropdown and the record browser can expand the value.
  defp fetch_foreign_keys!(table_name) do
    case Repo.query(
           """
           SELECT kcu.column_name, ccu.table_name AS referenced_table
           FROM information_schema.table_constraints tc
           JOIN information_schema.key_column_usage kcu
             ON tc.constraint_name = kcu.constraint_name
            AND tc.constraint_schema = kcu.constraint_schema
           JOIN information_schema.constraint_column_usage ccu
             ON ccu.constraint_name = tc.constraint_name
            AND ccu.constraint_schema = tc.constraint_schema
           WHERE tc.constraint_type = 'FOREIGN KEY'
             AND tc.table_schema = 'public'
             AND tc.table_name = $1
           """,
           [table_name]
         ) do
      {:ok, %{rows: rows}} ->
        Map.new(rows, fn [column_name, referenced] -> {column_name, referenced} end)

      _ ->
        %{}
    end
  end

  # Skips the implicit LazyPock columns (id/created_at/updated_at) and maps
  # Postgres column types to LazyPock field types. Best-effort: anything
  # unrecognized becomes "text" so the collection is still usable.
  #
  # Foreign-key columns become `relation` fields pointing at the referenced
  # table (`options.collection`), so the Studio renders a relation dropdown.
  defp infer_fields(columns, foreign_keys) do
    implicit = MapSet.new(["id", "created_at", "updated_at"])

    columns
    |> Enum.reject(&MapSet.member?(implicit, &1["column_name"]))
    |> Enum.map(fn col ->
      if referenced = Map.get(foreign_keys, col["column_name"]) do
        %{
          "name" => col["column_name"],
          "type" => "relation",
          "required" => col["is_nullable"] == "NO" and is_nil(col["column_default"]),
          "unique" => false,
          "options" => %{"collection" => referenced},
          "indexed" => false,
          "default" => nil
        }
      else
        %{
          "name" => col["column_name"],
          "type" => infer_field_type(col),
          "required" => col["is_nullable"] == "NO" and is_nil(col["column_default"]),
          "unique" => false,
          "options" => %{},
          "indexed" => false,
          "default" => nil
        }
      end
    end)
  end

  defp infer_field_type(col) do
    case {col["data_type"], col["udt_name"]} do
      {"boolean", _} -> "bool"
      {"smallint", _} -> "number"
      {"integer", _} -> "number"
      {"bigint", _} -> "number"
      {"numeric", _} -> "number"
      {"real", _} -> "number"
      {"double precision", _} -> "number"
      {"money", _} -> "number"
      {"date", _} -> "date"
      {"timestamp without time zone", _} -> "datetime"
      {"timestamp with time zone", _} -> "datetime"
      {"json", _} -> "json"
      {"jsonb", _} -> "json"
      {"ARRAY", "_text"} -> "multi_select"
      {"ARRAY", _} -> "json"
      # text, varchar, char, citext, uuid, enum, bytea, inet, ...
      {_, _} -> "text"
    end
  end

  # The `schema` array stored on `_collections` (mirrors DDL's shape).
  defp schema_entry(field) do
    %{
      "name" => field["name"],
      "type" => field["type"],
      "required" => field["required"],
      "unique" => field["unique"],
      "default" => field["default"],
      "options" => field["options"],
      "indexed" => field["indexed"]
    }
  end

  # Same default rules as DDL.create_collection/2 for base collections.
  defp default_base_rules do
    %{
      "listRule" => "",
      "viewRule" => "",
      "createRule" => "@request.auth.id != ''",
      "updateRule" => "",
      "deleteRule" => "",
      "manageRule" => nil
    }
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
