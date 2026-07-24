defmodule Lazypock.Collections.Collection do
  @moduledoc """
  Ecto schema for the `_collections` system table.

  Stores metadata about every collection (table), including built-in
  system collections like `_superusers`, `_mfas`, `_otps`, etc.
  """

  use Ecto.Schema

  import Ecto.Changeset

  # ── System collection names (matching PocketBase) ──────────

  @doc "_superusers collection (auth): admin superusers"
  def system_superusers, do: "_superusers"

  @doc "_externalAuths collection (base): OAuth2 external auth links"
  def system_external_auths, do: "_externalAuths"

  @doc "_mfas collection (base): multi-factor authentication records"
  def system_mfas, do: "_mfas"

  @doc "_otps collection (base): one-time password tokens"
  def system_otps, do: "_otps"

  @doc "_authOrigins collection (base): auth origin fingerprints"
  def system_auth_origins, do: "_authOrigins"

  @doc """
  Returns all known system collection names.
  """
  def system_names, do: [
    system_superusers(),
    system_external_auths(),
    system_mfas(),
    system_otps(),
    system_auth_origins()
  ]

  @doc """
  Returns true if the given collection name is a built-in system collection.
  """
  def system?(name), do: name in system_names()

  # ── Ecto schema ────────────────────────────────────────────

  @type t :: %__MODULE__{
          id: Ecto.UUID.t(),
          name: String.t(),
          type: String.t(),
          schema: map(),
          rules: map(),
          options: map(),
          hooks: map(),
          managed: boolean(),
          system: boolean(),
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
    field(:system, :boolean, default: false)

    has_many(:fields, Lazypock.Collections.Field, foreign_key: :collection_id)

    timestamps()
  end

  @doc false
  def changeset(struct, attrs \\ %{}) do
    struct
    |> cast(attrs, [:name, :type, :schema, :rules, :options, :hooks, :managed, :system])
    |> validate_required([:name, :type])
    |> validate_inclusion(:type, ["base", "auth"])
    |> validate_format(:name, ~r/^[a-z][a-z0-9_]*$/,
      message:
        "must start with a letter and contain only lowercase letters, numbers, and underscores"
    )
    |> unique_constraint(:name)
  end
end
