defmodule Lazypock.Repo.Migrations.FixUsersSystemFlag do
  use Ecto.Migration

  @doc """
  The built-in `users` auth collection was historically registered with
  `system = false` in `_collections` (only its fields were marked system).
  The canonical system-name set (`Lazypock.Collections.Collection.system?/1`)
  already treats it as system, and drop paths are protected by name too, but
  the DB flag should agree so exports/UI/metadata report it consistently and
  no code path that trusts the flag can drop it.
  """
  def up do
    execute("UPDATE _collections SET system = true WHERE name = 'users'")
  end

  def down do
    execute("UPDATE _collections SET system = false WHERE name = 'users'")
  end
end
