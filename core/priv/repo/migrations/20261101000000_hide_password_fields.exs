defmodule Lazypock.Repo.Migrations.HidePasswordFields do
  use Ecto.Migration

  @doc """
  Password fields are write-only (PocketBase parity):

  * they are **hidden** — never returned in API responses, never shown in
    the Studio record browser, and excluded from the SDK's default field
    projection;
  * they are **not required** — accounts can exist without a password (e.g.
    OAuth-only users or invite flows), so the backing columns are made
    nullable.

  The API key for the password remains `password` (aliased to the actual
  column, e.g. `password_hash`, by `DynamicController.sanitize_attrs/2`).
  """
  def up do
    # Mark every password-type field hidden + non-required (metadata only).
    execute """
    UPDATE _fields SET required = false, hidden = true WHERE type = 'password'
    """

    # Drop the NOT NULL constraint on every backing column so records can be
    # created without a password. Column = lowercased field name; collection
    # names are validated `^[a-z][a-z0-9_]*$`, and %I quoting keeps the
    # dynamic ALTER safe.
    execute """
    DO $$
    DECLARE
      r record;
    BEGIN
      FOR r IN
        SELECT c.name AS table_name, f.name AS field_name
        FROM _fields f
        JOIN _collections c ON c.id = f.collection_id
        WHERE f.type = 'password'
      LOOP
        EXECUTE format('ALTER TABLE %I ALTER COLUMN %I DROP NOT NULL',
          r.table_name, lower(r.field_name));
      END LOOP;
    END
    $$;
    """
  end

  def down do
    execute """
    UPDATE _fields SET required = true, hidden = false WHERE type = 'password'
    """

    execute """
    DO $$
    DECLARE
      r record;
    BEGIN
      FOR r IN
        SELECT c.name AS table_name, f.name AS field_name
        FROM _fields f
        JOIN _collections c ON c.id = f.collection_id
        WHERE f.type = 'password'
      LOOP
        EXECUTE format('ALTER TABLE %I ALTER COLUMN %I SET NOT NULL',
          r.table_name, lower(r.field_name));
      END LOOP;
    END
    $$;
    """
  end
end
