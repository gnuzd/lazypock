defmodule Lazypock.Repo do
  use Ecto.Repo,
    otp_app: :lazypock,
    adapter: Ecto.Adapters.Postgres

  @impl true
  def init(_type, config) do
    {:ok, Lazypock.Repo.SSL.apply(config)}
  end
end
