defmodule Lazypock.Repo.Migrations.FixSystemCollectionTableNames do
  use Ecto.Migration

  def up do
    # Fix table names from underscores to CamelCase
    execute "DROP TABLE IF EXISTS _external_auths CASCADE"
    execute "DROP TABLE IF EXISTS _auth_origins CASCADE"

    execute "CREATE TABLE IF NOT EXISTS _externalAuths (
      id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
      collection_ref TEXT NOT NULL,
      record_ref TEXT NOT NULL,
      provider TEXT NOT NULL,
      provider_id TEXT NOT NULL,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
      updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
    )"
    execute "CREATE UNIQUE INDEX IF NOT EXISTS idx_external_auths_record_provider ON _externalAuths (collection_ref, record_ref, provider)"
    execute "CREATE UNIQUE INDEX IF NOT EXISTS idx_external_auths_collection_provider ON _externalAuths (collection_ref, provider, provider_id)"

    execute "CREATE TABLE IF NOT EXISTS _authOrigins (
      id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
      collection_ref TEXT NOT NULL,
      record_ref TEXT NOT NULL,
      fingerprint TEXT NOT NULL,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
      updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
    )"
    execute "CREATE UNIQUE INDEX IF NOT EXISTS idx_auth_origins_unique_pairs ON _authOrigins (collection_ref, record_ref, fingerprint)"

    # Ensure system collections are registered
    execute """
      INSERT INTO _collections (name, type, system, managed, schema, rules, options, hooks, created_at, updated_at)
      SELECT '_externalAuths', 'base', true, false, '[]', '{}', '{}', '{}', now(), now()
      WHERE NOT EXISTS (SELECT 1 FROM _collections WHERE name = '_externalAuths')
    """
    execute """
      INSERT INTO _collections (name, type, system, managed, schema, rules, options, hooks, created_at, updated_at)
      SELECT '_mfas', 'base', true, false, '[]', '{}', '{}', '{}', now(), now()
      WHERE NOT EXISTS (SELECT 1 FROM _collections WHERE name = '_mfas')
    """
    execute """
      INSERT INTO _collections (name, type, system, managed, schema, rules, options, hooks, created_at, updated_at)
      SELECT '_otps', 'base', true, false, '[]', '{}', '{}', '{}', now(), now()
      WHERE NOT EXISTS (SELECT 1 FROM _collections WHERE name = '_otps')
    """
    execute """
      INSERT INTO _collections (name, type, system, managed, schema, rules, options, hooks, created_at, updated_at)
      SELECT '_authOrigins', 'base', true, false, '[]', '{}', '{}', '{}', now(), now()
      WHERE NOT EXISTS (SELECT 1 FROM _collections WHERE name = '_authOrigins')
    """

    # Mark _superusers and _collections as system if not already
    execute "UPDATE _collections SET system = true, managed = false WHERE name = '_superusers' AND (system IS NULL OR system = false)"
    execute "UPDATE _collections SET system = true, managed = false WHERE name = '_collections' AND (system IS NULL OR system = false)"
  end

  def down do
    execute "DROP TABLE IF EXISTS _externalAuths CASCADE"
    execute "DROP TABLE IF EXISTS _authOrigins CASCADE"
    execute "CREATE TABLE IF NOT EXISTS _external_auths (
      id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
      collection_ref TEXT NOT NULL,
      record_ref TEXT NOT NULL,
      provider TEXT NOT NULL,
      provider_id TEXT NOT NULL,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
      updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
    )"
    execute "CREATE UNIQUE INDEX IF NOT EXISTS idx_external_auths_record_provider ON _external_auths (collection_ref, record_ref, provider)"
    execute "CREATE UNIQUE INDEX IF NOT EXISTS idx_external_auths_collection_provider ON _external_auths (collection_ref, provider, provider_id)"
    execute "CREATE TABLE IF NOT EXISTS _auth_origins (
      id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
      collection_ref TEXT NOT NULL,
      record_ref TEXT NOT NULL,
      fingerprint TEXT NOT NULL,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
      updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
    )"
    execute "CREATE UNIQUE INDEX IF NOT EXISTS idx_auth_origins_unique_pairs ON _auth_origins (collection_ref, record_ref, fingerprint)"
  end
end
