defmodule Lazypock.Auth.SuperUser do
  @moduledoc """
  Ecto schema for the `_superusers` table.

  Internal table for super admin authentication — separate from
  dynamic user auth collections. Created on boot if it doesn't exist.
  """

  use Ecto.Schema
  import Ecto.Changeset

  @primary_key {:id, :binary_id, autogenerate: true}
  schema "_superusers" do
    field(:email, :string)
    field(:password_hash, :string)
    timestamps(inserted_at: :created_at, type: :utc_datetime)
  end

  def changeset(struct, attrs \\ %{}) do
    struct
    |> cast(attrs, [:email, :password_hash])
    |> validate_required([:email, :password_hash])
    |> validate_format(:email, ~r/^[^\s@]+@[^\s@]+$/, message: "invalid email")
    |> unique_constraint(:email)
  end
end
