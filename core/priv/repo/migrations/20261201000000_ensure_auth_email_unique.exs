defmodule Lazypock.Repo.Migrations.EnsureAuthEmailUnique do
  use Ecto.Migration

  @doc """
  Auth collections authenticate by a unique email (PocketBase parity).

  The built-in `users` table is created with an inline `email TEXT UNIQUE`
  constraint, but its `_fields` row never recorded the `unique` flag — so the
  Studio showed the email field as non-unique and a collection save could not
  re-assert it. Backfill the flag for every auth collection so the metadata
  matches the database. DDL now enforces it on create/update going forward.
  """
  def up do
    execute """
    UPDATE _fields f
    SET "unique" = true, updated_at = now()
    FROM _collections c
    WHERE f.collection_id = c.id
      AND c.type = 'auth'
      AND f.name = 'email'
      AND f."unique" = false
    """
  end

  def down do
    # One-way: the built-in users table already enforces uniqueness at the DB
    # level, so clearing the flag would only re-introduce the metadata/DB
    # mismatch this migration fixes.
    :ok
  end
end
