defmodule Lazypock.Repo.Migrations.FixSuperusersInsertedAt do
  use Ecto.Migration

  def up do
    # The _superusers table was created by DDL at boot time with inserted_at
    # Rename it to created_at to match the Ecto schema
    execute """
    DO $$
    BEGIN
      IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = '_superusers' AND column_name = 'inserted_at'
      ) THEN
        EXECUTE 'ALTER TABLE "_superusers" RENAME COLUMN "inserted_at" TO "created_at"';
      END IF;
    END $$;
    """
  end

  def down do
    execute """
    DO $$
    BEGIN
      IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = '_superusers' AND column_name = 'created_at'
      ) AND NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = '_superusers' AND column_name = 'inserted_at'
      ) THEN
        EXECUTE 'ALTER TABLE "_superusers" RENAME COLUMN "created_at" TO "inserted_at"';
      END IF;
    END $$;
    """
  end
end
