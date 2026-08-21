defmodule Lazypock.Repo.Migrations.CreateRequestLogsTable do
  use Ecto.Migration

  def up do
    create table(:_request_logs, primary_key: false) do
      add :id, :uuid, primary_key: true, default: fragment("gen_random_uuid()")
      add :method, :text, null: false
      add :path, :text, null: false
      add :status, :integer, null: false
      add :duration, :integer, null: false, default: 0
      add :ip, :text
      add :user_agent, :text
      add :referer, :text
      add :collection, :text
      add :error, :text

      add :created_at, :utc_datetime_usec, null: false, default: fragment("now()")
    end

    create index(:_request_logs, [:created_at])
    create index(:_request_logs, [:collection])
  end

  def down do
    drop table(:_request_logs)
  end
end
