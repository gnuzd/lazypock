-- 0003_rls.sql
--
-- Row-level security plumbing (PLAN.md section 6).
--
-- Two restricted roles are created for requests to run as, plus the helper
-- functions policies call to read the per-request context. Requests switch to a
-- restricted role with `SET LOCAL ROLE` inside their transaction, so the switch
-- is undone automatically at commit or rollback and cannot leak across a pooled
-- connection.
--
-- Note what is deliberately *not* here: `FORCE ROW LEVEL SECURITY`. Forcing it
-- would make the schema owner subject to policies too, which would break the
-- server's own internal reads (the login lookup being the obvious one) whenever
-- it connects as a non-superuser owner. The owner is the "service" identity:
-- internal work and superusers run as it and bypass policies, while every
-- user-facing request runs as one of the restricted roles below. That was
-- verified against Postgres: a superuser connection still has policies applied
-- to it once it does `SET LOCAL ROLE`.

-- Roles are cluster-scoped, so they are created once for the whole database
-- rather than per schema. Re-running is a no-op.
do $rls$
begin
    if not exists (select 1 from pg_roles where rolname = 'lazypock_anon') then
        create role lazypock_anon nologin;
    end if;
    if not exists (select 1 from pg_roles where rolname = 'lazypock_authenticated') then
        create role lazypock_authenticated nologin;
    end if;
exception
    when insufficient_privilege then
        raise exception using
            message = format(
                'lazypock needs to create the roles lazypock_anon and '
                'lazypock_authenticated, but the current role (%s) is not allowed to. '
                'Create them yourself (CREATE ROLE lazypock_anon NOLOGIN; '
                'CREATE ROLE lazypock_authenticated NOLOGIN;) and run migrations again.',
                current_user
            ),
            hint = 'See PLAN.md section 6 for the database roles row-level security needs.';
end
$rls$;

-- Helpers read the per-request context that the server sets with set_config.
create schema if not exists auth;

comment on schema auth is
    'Helpers for row-level security policies; reads the per-request context set by the server.';

create or replace function auth.uid() returns text
    language sql stable
    as $$ select nullif(current_setting('request.auth.id', true), '') $$;

create or replace function auth.collection_id() returns text
    language sql stable
    as $$ select nullif(current_setting('request.auth.collection_id', true), '') $$;

create or replace function auth.collection_name() returns text
    language sql stable
    as $$ select nullif(current_setting('request.auth.collection_name', true), '') $$;

create or replace function auth.claim(name text) returns text
    language sql stable
    as $$ select nullif(current_setting('request.auth.' || name, true), '') $$;

comment on function auth.uid() is 'The authenticated record id, or null when anonymous.';

-- The restricted roles need to reach the schema, the helper schema, and every
-- table a collection creates later. Tables are created by the connecting role,
-- so default privileges are what makes that hold without repeating a grant for
-- each collection.
do $grants$
declare
    target_schema text := current_schema();
begin
    execute format(
        'grant usage on schema %I to lazypock_anon, lazypock_authenticated',
        target_schema
    );
    execute format(
        'alter default privileges in schema %I '
        'grant select, insert, update, delete on tables to lazypock_anon, lazypock_authenticated',
        target_schema
    );
    execute format(
        'alter default privileges in schema %I '
        'grant usage, select on sequences to lazypock_anon, lazypock_authenticated',
        target_schema
    );

    -- Existing tables, for a database that already has collections.
    execute format(
        'grant select, insert, update, delete on all tables in schema %I '
        'to lazypock_anon, lazypock_authenticated',
        target_schema
    );
    execute format(
        'grant usage, select on all sequences in schema %I '
        'to lazypock_anon, lazypock_authenticated',
        target_schema
    );
end
$grants$;

grant usage on schema auth to lazypock_anon, lazypock_authenticated;
grant execute on all functions in schema auth to lazypock_anon, lazypock_authenticated;

alter default privileges in schema auth
    grant execute on functions to lazypock_anon, lazypock_authenticated;
