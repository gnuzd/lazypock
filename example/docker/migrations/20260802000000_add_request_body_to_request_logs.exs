defmodule Lazypock.Repo.Migrations.AddRequestBodyToRequestLogs do
  use Ecto.Migration

  def up do
    alter table(:_request_logs) do
      add :body, :text
    end
  end

  def down do
    alter table(:_request_logs) do
      remove :body
    end
  end
end
