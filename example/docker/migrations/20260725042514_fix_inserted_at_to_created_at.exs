defmodule Lazypock.Repo.Migrations.FixInsertedAtToCreatedAt do
  use Ecto.Migration

  def up do
    # Rename inserted_at to created_at for system tables that had old timestamps()
    # Uses DO block with exception handling so it's safe whether or not
    # the old migration had already run (for developers who ran old migrations
    # and also for clean installations where old migrations were already updated).
    safe_rename("_collections", "inserted_at", "created_at")
    safe_rename("_fields", "inserted_at", "created_at")
    safe_rename("_files", "inserted_at", "created_at")
  end

  def down do
    safe_rename("_collections", "created_at", "inserted_at")
    safe_rename("_fields", "created_at", "inserted_at")
    safe_rename("_files", "created_at", "inserted_at")
  end

  defp safe_rename(table, from_col, to_col) do
    execute """
    DO $$
    BEGIN
      IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = '#{table}' AND column_name = '#{from_col}'
      ) THEN
        EXECUTE 'ALTER TABLE "#{table}" RENAME COLUMN "#{from_col}" TO "#{to_col}"';
      END IF;
    END $$;
    """
  end
end
