defmodule Lazypock.Repo.Migrations.FixDefaultValueColumnType do
  use Ecto.Migration

  def up do
    execute "ALTER TABLE _fields ALTER COLUMN default_value TYPE TEXT USING default_value::text"
  end

  def down do
    execute "ALTER TABLE _fields ALTER COLUMN default_value TYPE JSONB USING default_value::jsonb"
  end
end