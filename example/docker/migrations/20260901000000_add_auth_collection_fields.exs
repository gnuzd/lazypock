defmodule Lazypock.Repo.Migrations.AddAuthCollectionFields do
  use Ecto.Migration

  def up do
    # Add columns to the users table for PocketBase-compatible auth features
    alter_table_if_not_exists("users")

    # Add fields to _fields table for the users collection
    add_system_fields("users", [
      %{name: "verified", type: "bool", required: true, system: true, sort_order: 6},
      %{name: "verificationToken", type: "text", system: true, sort_order: 7},
      %{name: "emailVisibility", type: "bool", required: true, system: true, sort_order: 8,
       options: %{defaultValue: true}}
    ])
  end

  def down do
    execute """
    DELETE FROM _fields WHERE name IN ('verified', 'verificationToken', 'emailVisibility')
    AND collection_id IN (SELECT id FROM _collections WHERE name = 'users')
    """
  end

  defp add_system_fields(collection_name, fields) do
    for field <- fields do
      system = Map.get(field, :system, true)
      required = Map.get(field, :required, false)
      opts = Map.get(field, :options, %{})
      opts_json = Jason.encode!(opts)
      name = field[:name]
      type = field[:type]
      sort_order = field[:sort_order]

      execute """
      INSERT INTO _fields (collection_id, name, type, required, system, sort_order, options, created_at, updated_at)
      SELECT c.id, '#{name}', '#{type}', #{required}, #{system}, #{sort_order}, '#{opts_json}'::jsonb, now(), now()
      FROM _collections c WHERE c.name = '#{collection_name}'
      AND NOT EXISTS (SELECT 1 FROM _fields f WHERE f.collection_id = c.id AND f.name = '#{name}')
      """
    end
  end

  defp alter_table_if_not_exists(table_name) do
    for {col, type, default} <- [
          {"verified", "BOOLEAN", "false"},
          {"verificationToken", "TEXT", nil},
          {"emailVisibility", "BOOLEAN", "true"}
        ] do
      default_clause = if default, do: " DEFAULT #{default}", else: ""

      execute """
      DO $$
      BEGIN
        IF NOT EXISTS (
          SELECT 1 FROM information_schema.columns
          WHERE table_name = '#{table_name}' AND column_name = '#{col}'
        ) THEN
          ALTER TABLE #{table_name} ADD COLUMN #{col} #{type}#{default_clause};
        END IF;
      END
      $$;
      """
    end
  end
end
