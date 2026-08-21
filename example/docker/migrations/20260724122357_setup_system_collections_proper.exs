defmodule Lazypock.Repo.Migrations.SetupSystemCollectionsProper do
  use Ecto.Migration

  def up do
    # ── Clean up wrong-named tables from earlier migrations ──
    execute "DROP TABLE IF EXISTS _externalAuths CASCADE"
    execute "DROP TABLE IF EXISTS _authOrigins CASCADE"

    # ── Remove stale entries with old CamelCase names ──
    execute "DELETE FROM _collections WHERE name = '_externalAuths'"
    execute "DELETE FROM _collections WHERE name = '_authOrigins'"

    # ── Create system tables with snake_case names ──

    # _external_auths
    execute "CREATE TABLE IF NOT EXISTS _external_auths (
      id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
      collection TEXT NOT NULL,
      provider TEXT NOT NULL,
      provider_id TEXT NOT NULL,
      user_id UUID NOT NULL,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
      updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
    )"
    execute "CREATE UNIQUE INDEX IF NOT EXISTS idx_external_auths_collection_provider ON _external_auths (collection, provider, provider_id)"

    # _mfas
    execute "CREATE TABLE IF NOT EXISTS _mfas (
      id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
      collection_ref TEXT NOT NULL,
      record_ref TEXT NOT NULL,
      method TEXT NOT NULL,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
      updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
    )"
    execute "CREATE INDEX IF NOT EXISTS idx_mfas_collection_ref_record_ref ON _mfas (collection_ref, record_ref)"

    # _otps
    execute "CREATE TABLE IF NOT EXISTS _otps (
      id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
      collection_ref TEXT NOT NULL,
      record_ref TEXT NOT NULL,
      password_hash TEXT NOT NULL,
      sent_to TEXT,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
      updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
    )"
    execute "CREATE INDEX IF NOT EXISTS idx_otps_collection_ref_record_ref ON _otps (collection_ref, record_ref)"

    # _auth_origins
    execute "CREATE TABLE IF NOT EXISTS _auth_origins (
      id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
      collection_ref TEXT NOT NULL,
      record_ref TEXT NOT NULL,
      fingerprint TEXT NOT NULL,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
      updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
    )"
    execute "CREATE UNIQUE INDEX IF NOT EXISTS idx_auth_origins_unique_pairs ON _auth_origins (collection_ref, record_ref, fingerprint)"

    # ── Register system collections in _collections ──
    register_system_collection("_external_auths", "base")
    register_system_collection("_mfas", "base")
    register_system_collection("_otps", "base")
    register_system_collection("_auth_origins", "base")

    # Ensure _superusers is registered as system auth collection
    execute """
      INSERT INTO _collections (name, type, system, managed, schema, rules, options, hooks, created_at, updated_at)
      SELECT '_superusers', 'auth', true, false, '[]', '{}', '{}', '{}', now(), now()
      WHERE NOT EXISTS (SELECT 1 FROM _collections WHERE name = '_superusers')
    """

    # Create _superusers fields in _fields (table already exists from Auth.Setup)
    execute """
      INSERT INTO _fields (collection_id, name, type, required, system, sort_order, options, created_at, updated_at)
      SELECT c.id, 'email', 'email', true, true, 0, '{}'::jsonb, now(), now()
      FROM _collections c WHERE c.name = '_superusers'
      AND NOT EXISTS (SELECT 1 FROM _fields f WHERE f.collection_id = c.id AND f.name = 'email')
    """
    execute """
      INSERT INTO _fields (collection_id, name, type, required, system, sort_order, options, created_at, updated_at)
				SELECT c.id, 'password_hash', 'password', true, true, 1, '{}'::jsonb, now(), now()
      FROM _collections c WHERE c.name = '_superusers'
      AND NOT EXISTS (SELECT 1 FROM _fields f WHERE f.collection_id = c.id AND f.name = 'password_hash')
    """
    execute """
      INSERT INTO _fields (collection_id, name, type, required, system, sort_order, options, created_at, updated_at)
      SELECT c.id, 'created_at', 'autodate', true, true, 2, '{\"onCreate\": true}'::jsonb, now(), now()
      FROM _collections c WHERE c.name = '_superusers'
      AND NOT EXISTS (SELECT 1 FROM _fields f WHERE f.collection_id = c.id AND f.name = 'created_at')
    """
    execute """
      INSERT INTO _fields (collection_id, name, type, required, system, sort_order, options, created_at, updated_at)
      SELECT c.id, 'updated_at', 'autodate', true, true, 3, '{\"onCreate\": true, \"onUpdate\": true}'::jsonb, now(), now()
      FROM _collections c WHERE c.name = '_superusers'
      AND NOT EXISTS (SELECT 1 FROM _fields f WHERE f.collection_id = c.id AND f.name = 'updated_at')
    """

    # ── Create users auth collection ──
    execute "CREATE TABLE IF NOT EXISTS users (
      id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
      email TEXT UNIQUE NOT NULL,
      password_hash TEXT NOT NULL,
      avatar TEXT,
      name TEXT,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
      updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
    )"

    execute """
      INSERT INTO _collections (name, type, system, managed, schema, rules, options, hooks, created_at, updated_at)
      SELECT 'users', 'auth', false, true, '[]',
        '{"listRule": "", "viewRule": "", "createRule": "", "updateRule": "id = @request.auth.id", "deleteRule": "id = @request.auth.id"}'::jsonb,
        '{}', '{}', now(), now()
      WHERE NOT EXISTS (SELECT 1 FROM _collections WHERE name = 'users')
    """

    execute """
      INSERT INTO _fields (collection_id, name, type, required, system, sort_order, options, created_at, updated_at)
      SELECT c.id, 'email', 'email', true, true, 0, '{}'::jsonb, now(), now()
      FROM _collections c WHERE c.name = 'users'
      AND NOT EXISTS (SELECT 1 FROM _fields f WHERE f.collection_id = c.id AND f.name = 'email')
    """
    execute """
      INSERT INTO _fields (collection_id, name, type, required, system, sort_order, options, created_at, updated_at)
				SELECT c.id, 'password_hash', 'password', true, true, 1, '{}'::jsonb, now(), now()
      FROM _collections c WHERE c.name = 'users'
      AND NOT EXISTS (SELECT 1 FROM _fields f WHERE f.collection_id = c.id AND f.name = 'password_hash')
    """


    execute """
      INSERT INTO _fields (collection_id, name, type, required, system, sort_order, options, created_at, updated_at)
      SELECT c.id, 'created_at', 'autodate', true, true, 2, '{\"onCreate\": true}'::jsonb, now(), now()
      FROM _collections c WHERE c.name = 'users'
      AND NOT EXISTS (SELECT 1 FROM _fields f WHERE f.collection_id = c.id AND f.name = 'created_at')
    """
    execute """
      INSERT INTO _fields (collection_id, name, type, required, system, sort_order, options, created_at, updated_at)
      SELECT c.id, 'updated_at', 'autodate', true, true, 3, '{\"onCreate\": true, \"onUpdate\": true}'::jsonb, now(), now()
      FROM _collections c WHERE c.name = 'users'
      AND NOT EXISTS (SELECT 1 FROM _fields f WHERE f.collection_id = c.id AND f.name = 'updated_at')
    """
    execute """
      INSERT INTO _fields (collection_id, name, type, required, system, sort_order, options, created_at, updated_at)
      SELECT c.id, 'name', 'text', false, false, 4, '{\"max\": 255}'::jsonb, now(), now()
      FROM _collections c WHERE c.name = 'users'
      AND NOT EXISTS (SELECT 1 FROM _fields f WHERE f.collection_id = c.id AND f.name = 'name')
    """
    execute """
      INSERT INTO _fields (collection_id, name, type, required, system, sort_order, options, created_at, updated_at)
      SELECT c.id, 'avatar', 'file', false, false, 5, '{\"maxSelect\": 1, \"mimeTypes\": [\"image/jpeg\",\"image/png\",\"image/svg+xml\",\"image/gif\",\"image/webp\"]}'::jsonb, now(), now()
      FROM _collections c WHERE c.name = 'users'
      AND NOT EXISTS (SELECT 1 FROM _fields f WHERE f.collection_id = c.id AND f.name = 'avatar')
    """

    # ── Mark _collections itself as system ──
    execute "UPDATE _collections SET system = true, managed = false WHERE name = '_collections' AND (system IS NULL OR system = false)"
  end

  def down do
    execute "DROP TABLE IF EXISTS _external_auths CASCADE"
    execute "DROP TABLE IF EXISTS _mfas CASCADE"
    execute "DROP TABLE IF EXISTS _otps CASCADE"
    execute "DROP TABLE IF EXISTS _auth_origins CASCADE"
    execute "DROP TABLE IF EXISTS users CASCADE"
  end

  defp register_system_collection(name, type) do
    execute """
      INSERT INTO _collections (name, type, system, managed, schema, rules, options, hooks, created_at, updated_at)
      SELECT '#{name}', '#{type}', true, false, '[]', '{}', '{}', '{}', now(), now()
      WHERE NOT EXISTS (SELECT 1 FROM _collections WHERE name = '#{name}')
    """
  end
end
