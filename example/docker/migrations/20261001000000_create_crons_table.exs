defmodule Lazypock.Repo.Migrations.CreateCronsTable do
  use Ecto.Migration

  def up do
    create table(:_crons, primary_key: false) do
      add :id, :uuid, primary_key: true, default: fragment("gen_random_uuid()")
      add :name, :text, null: false
      # Cron expression — standard 5-field or PocketBase-style 6-field (seconds).
      add :expression, :text, null: false
      # IANA time zone name defining the wall-clock context of `expression`
      # (e.g. "UTC", "America/New_York"). Stored timestamps stay UTC.
      add :timezone, :text, null: false, default: "UTC"
      add :enabled, :boolean, null: false, default: true
      # Action type: "sql" | "http" | "hook"
      add :action, :text, null: false, default: "http"
      # Action-specific configuration (method/url/headers/body for http,
      # statement for sql, event name for hook).
      add :config, :jsonb, null: false, default: "{}"

      # Last run bookkeeping (updated by the scheduler / run-now).
      add :last_run_at, :utc_datetime_usec
      add :last_status, :text
      add :last_duration_ms, :integer
      add :last_error, :text
      add :next_run_at, :utc_datetime_usec

      add :created_at, :utc_datetime_usec, null: false, default: fragment("now()")
      add :updated_at, :utc_datetime_usec, null: false, default: fragment("now()")
    end

    create unique_index(:_crons, [:name])
    create index(:_crons, [:enabled])
    create index(:_crons, [:next_run_at])
  end

  def down do
    drop table(:_crons)
  end
end
