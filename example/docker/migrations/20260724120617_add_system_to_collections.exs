defmodule Lazypock.Repo.Migrations.AddSystemToCollections do
  use Ecto.Migration

  def up do
    # Phase 1: Add system column to _collections
    alter table(:_collections) do
      add :system, :boolean, default: false, null: false
    end

    # Phase 2: Create system collection tables and register them

    # -- _externalAuths --
    execute "CREATE TABLE IF NOT EXISTS _externalAuths (
      id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
      collection TEXT NOT NULL,
      provider TEXT NOT NULL,
      provider_id TEXT NOT NULL,
      user_id UUID NOT NULL,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
      updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
    )"
    execute "CREATE UNIQUE INDEX IF NOT EXISTS idx_external_auths_collection_provider ON _externalAuths (collection, provider, provider_id)"

    execute """
      INSERT INTO _collections (name, type, system, managed, schema, rules, options, hooks, created_at, updated_at)
      SELECT '_externalAuths', 'base', true, false, '[]',
        '{}', '{}', '{}', now(), now()
      WHERE NOT EXISTS (SELECT 1 FROM _collections WHERE name = '_externalAuths')
    """

    # -- _mfas --
    execute "CREATE TABLE IF NOT EXISTS _mfas (
      id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
      collection_ref TEXT NOT NULL,
      record_ref TEXT NOT NULL,
      method TEXT NOT NULL,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
      updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
    )"
    execute "CREATE INDEX IF NOT EXISTS idx_mfas_collection_ref_record_ref ON _mfas (collection_ref, record_ref)"

    execute """
      INSERT INTO _collections (name, type, system, managed, schema, rules, options, hooks, created_at, updated_at)
      SELECT '_mfas', 'base', true, false, '[]',
        '{}', '{}', '{}', now(), now()
      WHERE NOT EXISTS (SELECT 1 FROM _collections WHERE name = '_mfas')
    """

    # -- _otps --
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

    execute """
      INSERT INTO _collections (name, type, system, managed, schema, rules, options, hooks, created_at, updated_at)
      SELECT '_otps', 'base', true, false, '[]',
        '{}', '{}', '{}', now(), now()
      WHERE NOT EXISTS (SELECT 1 FROM _collections WHERE name = '_otps')
    """

    # -- _authOrigins --
    execute "CREATE TABLE IF NOT EXISTS _authOrigins (
      id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
      collection_ref TEXT NOT NULL,
      record_ref TEXT NOT NULL,
      fingerprint TEXT NOT NULL,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
      updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
    )"
    execute "CREATE UNIQUE INDEX IF NOT EXISTS idx_auth_origins_unique_pairs ON _authOrigins (collection_ref, record_ref, fingerprint)"

    execute """
      INSERT INTO _collections (name, type, system, managed, schema, rules, options, hooks, created_at, updated_at)
      SELECT '_authOrigins', 'base', true, false, '[]',
        '{}', '{}', '{}', now(), now()
      WHERE NOT EXISTS (SELECT 1 FROM _collections WHERE name = '_authOrigins')
    """

    # -- Mark _superusers as system --
    execute """
      UPDATE _collections SET system = true, managed = false
      WHERE name = '_superusers'
    """

    # -- Mark _collections itself as system --
    execute """
      UPDATE _collections SET system = true, managed = false
      WHERE name = '_collections'
    """
  end

  def down do
    alter table(:_collections) do
      remove :system
    end

    execute "DROP TABLE IF EXISTS _externalAuths CASCADE"
    execute "DROP TABLE IF EXISTS _mfas CASCADE"
    execute "DROP TABLE IF EXISTS _otps CASCADE"
    execute "DROP TABLE IF EXISTS _authOrigins CASCADE"
  end
end
