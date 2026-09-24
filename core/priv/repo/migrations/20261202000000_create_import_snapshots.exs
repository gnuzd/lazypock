defmodule Lazypock.Repo.Migrations.CreateImportSnapshots do
  use Ecto.Migration

  @doc """
  Stores a point-in-time snapshot (a full `Backup.export/0` payload) taken
  right before each import/restore, so a bad import can be rolled back to the
  state it replaced. The Studio exposes this as "Undo last import".
  """
  def up do
    execute """
    CREATE TABLE IF NOT EXISTS _import_snapshots (
      id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
      payload JSONB NOT NULL
    )
    """

    execute """
    CREATE INDEX IF NOT EXISTS _import_snapshots_created_at_idx
    ON _import_snapshots (created_at DESC)
    """
  end

  def down do
    execute "DROP TABLE IF EXISTS _import_snapshots"
  end
end
