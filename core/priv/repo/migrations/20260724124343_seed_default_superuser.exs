defmodule Lazypock.Repo.Migrations.SeedDefaultSuperuser do
  use Ecto.Migration

  # Pre-compute hash via Bcrypt at migration time — Bcrypt is a pure Elixir dep
  # so it works without the app being started
  @default_password_hash Bcrypt.hash_pwd_salt("password")

  def up do
    execute """
    INSERT INTO _superusers (email, password_hash)
    SELECT 'admin@example.com', '#{@default_password_hash}'
    WHERE NOT EXISTS (SELECT 1 FROM _superusers)
    """
  end

  def down do
    execute "DELETE FROM _superusers WHERE email = 'admin@example.com'"
  end
end
