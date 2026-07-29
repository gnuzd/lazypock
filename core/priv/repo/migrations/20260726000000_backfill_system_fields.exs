defmodule Lazypock.Repo.Migrations.BackfillSystemFields do
  use Ecto.Migration

  def up do
    # ── _external_auths fields ──
    add_system_fields("_external_auths", [
      %{name: "collection_ref", type: "text", required: true, sort_order: 0},
      %{name: "record_ref", type: "text", required: true, sort_order: 1},
      %{name: "provider", type: "text", required: true, sort_order: 2},
      %{name: "provider_id", type: "text", required: true, sort_order: 3},
      %{name: "created_at", type: "autodate", options: %{onCreate: true}, sort_order: 4},
      %{name: "updated_at", type: "autodate", options: %{onCreate: true, onUpdate: true}, sort_order: 5}
    ])

    # ── _mfas fields ──
    add_system_fields("_mfas", [
      %{name: "collection_ref", type: "text", required: true, sort_order: 0},
      %{name: "record_ref", type: "text", required: true, sort_order: 1},
      %{name: "method", type: "text", required: true, sort_order: 2},
      %{name: "created_at", type: "autodate", options: %{onCreate: true}, sort_order: 3},
      %{name: "updated_at", type: "autodate", options: %{onCreate: true, onUpdate: true}, sort_order: 4}
    ])

    # ── _otps fields ──
    add_system_fields("_otps", [
      %{name: "collection_ref", type: "text", required: true, sort_order: 0},
      %{name: "record_ref", type: "text", required: true, sort_order: 1},
      %{name: "password_hash", type: "password", required: true, sort_order: 2},
      %{name: "sent_to", type: "text", sort_order: 3},
      %{name: "created_at", type: "autodate", options: %{onCreate: true}, sort_order: 4},
      %{name: "updated_at", type: "autodate", options: %{onCreate: true, onUpdate: true}, sort_order: 5}
    ])

    # ── _auth_origins fields ──
    add_system_fields("_auth_origins", [
      %{name: "collection_ref", type: "text", required: true, sort_order: 0},
      %{name: "record_ref", type: "text", required: true, sort_order: 1},
      %{name: "fingerprint", type: "text", required: true, sort_order: 2},
      %{name: "created_at", type: "autodate", options: %{onCreate: true}, sort_order: 3},
      %{name: "updated_at", type: "autodate", options: %{onCreate: true, onUpdate: true}, sort_order: 4}
    ])

    # ── users: add missing columns and fields ──
    # Add columns to users table (safe: IF NOT EXISTS / column already exists check)
    alter_table_if_not_exists("users")

    add_system_fields("users", [
      %{name: "email", type: "email", required: true, system: true, sort_order: 0},
      %{name: "password_hash", type: "password", required: true, system: true, sort_order: 1},
      %{name: "name", type: "text", system: false, sort_order: 2},
      %{name: "avatar", type: "file", system: false, sort_order: 3, options: %{maxSelect: 1, mimeTypes: ["image/jpeg", "image/png", "image/svg+xml", "image/gif", "image/webp"]}},
      %{name: "created_at", type: "autodate", system: true, options: %{onCreate: true}, sort_order: 4},
      %{name: "updated_at", type: "autodate", system: true, options: %{onCreate: true, onUpdate: true}, sort_order: 5}
    ])
  end

  def down do
    # Remove fields we added — safe to re-run up
    execute "DELETE FROM _fields WHERE name IN ('collection_ref', 'record_ref', 'provider', 'provider_id', 'method', 'password_hash', 'sent_to', 'fingerprint', 'email', 'name', 'avatar') AND collection_id IN (SELECT id FROM _collections WHERE system = true)"
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
    # Add columns that _superusers has but users may be missing
    for {col, type, default} <- [
          {"email", "TEXT", nil},
          {"password_hash", "TEXT", nil},
          {"name", "TEXT", nil},
          {"avatar", "TEXT", nil},
          {"created_at", "TIMESTAMPTZ", "now()"},
          {"updated_at", "TIMESTAMPTZ", "now()"}
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
