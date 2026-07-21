defmodule Lazypock.Collections.Collection do
  @moduledoc """
  Ecto schema for the `_collections` system table.

  Stores metadata about every user-created collection (table).
  Each collection has a name (becomes the PostgreSQL table name),
  a type (base or auth), rules for access control, hooks configuration,
  and other options.
  """

  use Ecto.Schema

  import Ecto.Changeset

  @type t :: %__MODULE__{
          id: Ecto.UUID.t(),
          name: String.t(),
          type: String.t(),
          schema: map(),
          rules: map(),
          options: map(),
          hooks: map(),
          managed: boolean(),
          fields: [Lazypock.Collections.Field.t()] | Ecto.Association.NotLoaded.t()
        }

  @primary_key {:id, :binary_id, autogenerate: true}
  schema "_collections" do
    field(:name, :string)
    field(:type, :string, default: "base")
    field(:schema, {:array, :map}, default: [])
    field(:rules, :map, default: %{})
    field(:options, :map, default: %{})
    field(:hooks, :map, default: %{})
    field(:managed, :boolean, default: true)

    has_many(:fields, Lazypock.Collections.Field, foreign_key: :collection_id)

    timestamps()
  end

  @doc false
  def changeset(struct, attrs \\ %{}) do
    struct
    |> cast(attrs, [:name, :type, :schema, :rules, :options, :hooks, :managed])
    |> validate_required([:name, :type])
    |> validate_inclusion(:type, ["base", "auth"])
    |> validate_format(:name, ~r/^[a-z][a-z0-9_]*$/,
      message:
        "must start with a letter and contain only lowercase letters, numbers, and underscores"
    )
    |> unique_constraint(:name)
  end
end
