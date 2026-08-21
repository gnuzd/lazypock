defmodule Lazypock.Repo.Migrations.AddExampleNotesTable do
  @moduledoc """
  Example user migration.

  This folder is mounted at `LAZYPOCK_MIGRATIONS_DIR` (/migrations) by
  example/docker/compose.yml. The server applies any `NNNN..._name.exs`
  files found there on boot (or run `lazypock migrate` to apply them
  manually). Drop in your own files with a LATER timestamp to add more.

  After `docker compose up`, this creates the `example_notes` table —
  check it with:

      psql postgres://postgres:postgres@localhost:5432/lazypock \
        -c '\d example_notes'
  """
  use Ecto.Migration

  def up do
    create table(:example_notes) do
      add :title, :text, null: false
      add :body, :text
      add :published, :boolean, default: false, null: false

      timestamps()
    end

    create index(:example_notes, [:published])
  end

  def down do
    drop table(:example_notes)
  end
end
