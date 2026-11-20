-- 0020_revoke_system_tables.sql
--
-- Migration 0003 grants select/insert/update/delete to lazypock_anon and
-- lazypock_authenticated on *every* table in the schema, and (through default
-- privileges) on every table created later. That is what lets collection tables
-- work without a grant per collection, but it also covers the internal `_` tables:
-- after 0001-0019 the restricted roles can still read and write `_superusers`
-- (password hashes), `_settings` (signing key), `_sessions`, `_api_keys`,
-- `_collections`, `_fields`, `_jobs`, `_logs`, `_audit`, ... with row-level security
-- off. Nothing in the server is meant to reach them as a restricted role; this
-- only matters if a bug ever lets request-supplied SQL run as one, but then it
-- is the difference between "reads one collection" and "reads the credentials".
--
-- 0018 and 0019 already did this for `_rt_log` and `_mail_templates`; this
-- finishes the job for the rest.
--
-- Kept reachable on purpose (they are protected by their own RLS policies):
--   _files, _file_refs, _image_presets
--
-- Convention from here on: a migration that adds an internal `_` table ends with
--   revoke all on <table> from lazypock_anon, lazypock_authenticated;
-- because default privileges will have granted it automatically.

do $mig$
declare
    target_schema text := current_schema();
    obj record;
begin
    for obj in
        select c.relname
          from pg_class c
          join pg_namespace n on n.oid = c.relnamespace
         where n.nspname = target_schema
           and c.relkind in ('r', 'p')
           and c.relname like '\_%'
           and c.relname not in ('_files', '_file_refs', '_image_presets')
    loop
        execute format(
            'revoke all on table %I.%I from lazypock_anon, lazypock_authenticated',
            target_schema, obj.relname
        );
    end loop;

    -- Sequences behind those tables (`_logs_id_seq`, `_audit_id_seq`, ...).
    for obj in
        select c.relname
          from pg_class c
          join pg_namespace n on n.oid = c.relnamespace
         where n.nspname = target_schema
           and c.relkind = 'S'
           and c.relname like '\_%'
    loop
        execute format(
            'revoke all on sequence %I.%I from lazypock_anon, lazypock_authenticated',
            target_schema, obj.relname
        );
    end loop;
end
$mig$;
