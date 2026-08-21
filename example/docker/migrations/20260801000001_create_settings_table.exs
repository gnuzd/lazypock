defmodule Lazypock.Repo.Migrations.CreateSettingsTable do
  use Ecto.Migration

  def up do
    create table(:_settings, primary_key: false) do
      add :id, :uuid, primary_key: true, default: fragment("gen_random_uuid()")
      # Single row stores the entire settings blob as JSONB
      add :data, :jsonb, null: false, default: "{}"

      add :created_at, :utc_datetime_usec, null: false, default: fragment("now()")
      add :updated_at, :utc_datetime_usec, null: false, default: fragment("now()")
    end
  end

  def down do
    drop table(:_settings)
  end
end
