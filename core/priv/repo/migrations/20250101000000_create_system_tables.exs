defmodule Lazypock.Repo.Migrations.CreateSystemTables do
  use Ecto.Migration

  def up do
    create table(:_collections, primary_key: false) do
      add :id, :uuid, primary_key: true, default: fragment("gen_random_uuid()")
      add :name, :text, null: false
      add :type, :text, null: false, default: "base"
      add :schema, :jsonb, null: false, default: "[]"
      add :rules, :jsonb, null: false, default: "{}"
      add :options, :jsonb, null: false, default: "{}"
      add :hooks, :jsonb, null: false, default: "{}"
      add :managed, :boolean, null: false, default: true

      add :created_at, :utc_datetime_usec, null: false, default: fragment("now()")
      add :updated_at, :utc_datetime_usec, null: false, default: fragment("now()")
    end

    create unique_index(:_collections, [:name])

    create table(:_fields, primary_key: false) do
      add :id, :uuid, primary_key: true, default: fragment("gen_random_uuid()")
      add :collection_id, references(:_collections, type: :uuid, on_delete: :delete_all), null: false
      add :name, :text, null: false
      add :type, :text, null: false
      add :required, :boolean, default: false
      add :unique, :boolean, default: false
      add :default_value, :jsonb
      add :options, :jsonb, default: "{}"
      add :indexed, :boolean, default: false
      add :hidden, :boolean, default: false
      add :system, :boolean, default: false
      add :sort_order, :integer, default: 0

      add :created_at, :utc_datetime_usec, null: false, default: fragment("now()")
      add :updated_at, :utc_datetime_usec, null: false, default: fragment("now()")
    end

    create unique_index(:_fields, [:collection_id, :name])
    create index(:_fields, [:collection_id])
  end

  def down do
    drop table(:_fields)
    drop table(:_collections)
  end
end
