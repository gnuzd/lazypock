defmodule Lazypock.Collections.Field do
  @moduledoc """
  Ecto schema for the `_fields` system table.

  Stores metadata about each field (column) in a user-created collection.
  Fields have a name, type (text, number, bool, etc.), validation rules,
  and display options.
  """

  use Ecto.Schema

  import Ecto.Changeset

  @type t :: %__MODULE__{
          id: Ecto.UUID.t(),
          collection_id: Ecto.UUID.t(),
          name: String.t(),
          type: String.t(),
          required: boolean(),
          unique: boolean(),
          default_value: any(),
          options: map(),
          indexed: boolean(),
          hidden: boolean(),
          system: boolean(),
          sort_order: integer()
        }

  @valid_types ~w(text number bool email url date datetime select multi_select file multi_file json relation editor password geo)

  @primary_key {:id, :binary_id, autogenerate: true}
  schema "_fields" do
    belongs_to(:collection, Lazypock.Collections.Collection, type: :binary_id)

    field(:name, :string)
    field(:type, :string)
    field(:required, :boolean, default: false)
    field(:unique, :boolean, default: false)
    field(:default_value, Lazypock.Ecto.JSONValue)
    field(:options, :map, default: %{})
    field(:indexed, :boolean, default: false)
    field(:hidden, :boolean, default: false)
    field(:system, :boolean, default: false)
    field(:sort_order, :integer, default: 0)

    timestamps(inserted_at: :created_at, type: :utc_datetime)
  end

  @doc false
  def changeset(struct, attrs \\ %{}) do
    struct
    |> cast(attrs, [
      :collection_id,
      :name,
      :type,
      :required,
      :unique,
      :default_value,
      :options,
      :indexed,
      :hidden,
      :system,
      :sort_order
    ])
    |> validate_required([:collection_id, :name, :type])
    |> validate_inclusion(:type, @valid_types,
      message: "must be one of: #{Enum.join(@valid_types, ", ")}"
    )
    |> validate_format(:name, ~r/^[a-z][a-z0-9_]*$/,
      message:
        "must start with a letter and contain only lowercase letters, numbers, and underscores"
    )
    |> unique_constraint(:name, name: :fields_collection_id_name_index)
    |> foreign_key_constraint(:collection_id)
  end
end
