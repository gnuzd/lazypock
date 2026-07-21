defmodule Lazypock.Repo do
  use Ecto.Repo,
    otp_app: :lazypock,
    adapter: Ecto.Adapters.Postgres
end
