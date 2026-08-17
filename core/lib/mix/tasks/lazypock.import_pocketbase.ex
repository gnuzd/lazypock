defmodule Mix.Tasks.Lazypock.ImportPocketbase do
  @moduledoc """
  Imports a PocketBase instance into LazyPock.

  Reads a PocketBase SQLite database (`pb_data/data.db`, or `--pb-db`) plus its
  `storage/` directory, and imports collections, records, auth users, OAuth
  links and files into the running LazyPock/Postgres instance.

  ## Usage

      mix lazypock.import_pocketbase --pb-dir=./pb_data [--yes] [--dry-run]
      mix lazypock.import_pocketbase --pb-db=/path/to/data.db --storage-dir=/path/to/storage

  ## Options

    * `--pb-dir` — path to a PocketBase `pb_data` directory (default `pb_data`)
    * `--pb-db` — direct path to the SQLite `data.db` (overrides `--pb-dir`)
    * `--storage-dir` — path to the PocketBase `storage` dir
      (default `<pb_data>/storage`)
    * `--dry-run` — print what would be imported without changing anything
    * `--yes` — skip the confirmation prompt; also allows importing records
      into collections that already exist in LazyPock
    * `--id-map-file` — where to write the old-id → new-id JSON mapping
      (default `pocketbase_id_map.json`)

  PocketBase record ids are rewritten to deterministic UUIDv5 ids so relations
  stay intact. See `Lazypock.PocketBase.Importer` for details.
  """

  use Mix.Task

  @shortdoc "Import a PocketBase instance (collections, records, files) into LazyPock"

  @impl true
  def run(args) do
    Mix.Task.run("app.start")

    {opts, _} =
      OptionParser.parse!(args,
        strict: [
          pb_dir: :string,
          pb_db: :string,
          storage_dir: :string,
          dry_run: :boolean,
          yes: :boolean,
          id_map_file: :string
        ],
        aliases: [y: :yes, d: :dry_run]
      )

    opts = Enum.map(opts, fn {k, v} -> {k, v || true} end)

    confirm!(opts)

    summary = Lazypock.PocketBase.Importer.import_all(opts)

    IO.puts("\n── Import summary ─────────────────────────")
    IO.puts("  dry run:       #{summary[:dry_run]}")
    IO.puts("  collections:   #{summary[:collections] || 0}")
    IO.puts("  records:       #{summary[:records] || 0}")
    IO.puts("  files:         #{summary[:files] || 0}")
    IO.puts("  external auth: #{summary[:external_auths] || 0}")
    IO.puts("  id map file:   #{summary[:id_map_file] || "—"}")

    if summary[:dry_run] do
      IO.puts("  would import:  #{Enum.join(summary[:collections_list], ", ")}")
    end

    case summary[:warnings] do
      [] -> :ok
      warnings ->
        IO.puts("\n── Warnings ───────────────────────────────")
        Enum.each(warnings, &IO.puts("  ! #{&1}"))
    end

    IO.puts("───────────────────────────────────────────")
  end

  defp confirm!(opts) do
    if opts[:dry_run] do
      :ok
    else
      if opts[:yes] do
        :ok
      else
        answer =
          IO.gets(
            "This will create collections and records in the LazyPock database. " <>
              "Continue? [y/N] "
          ) || ""

        unless String.trim(answer) in ["y", "Y", "yes"] do
          Mix.raise("Aborted.")
        end
      end
    end
  end
end
