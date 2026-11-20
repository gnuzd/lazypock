-- 0019_grant_roles_to_owner.sql
--
-- Requests run with `SET LOCAL ROLE lazypock_anon | lazypock_authenticated`.
-- Migration 0003 creates those roles but never makes the connecting role a
-- member of them. A superuser can SET ROLE to anything, so that goes unnoticed;
-- a regular owner (the normal case on managed Postgres) cannot. Since
-- PostgreSQL 16 even the role that *created* them only receives ADMIN OPTION,
-- not the right to SET ROLE, so every request would fail with
-- "permission denied to set role".
--
-- This grants membership when it is missing and leaves it alone when SET ROLE
-- already works (superuser, or an administrator pre-created and granted the
-- roles, as the hint in 0003 suggests). Re-running is a no-op.

do $mig$
declare
    r text;
begin
    foreach r in array array['lazypock_anon', 'lazypock_authenticated'] loop
        -- Already able to switch? Then there is nothing to do.
        begin
            execute format('set local role %I', r);
            execute 'reset role';
            continue;
        exception
            when insufficient_privilege then
                null;
        end;

        begin
            execute format('grant %I to %I', r, current_user);
            execute format('set local role %I', r);
            execute 'reset role';
        exception
            when insufficient_privilege then
                raise exception using
                    message = format(
                        'the current role (%s) is not allowed to SET ROLE %s, and cannot grant itself that right',
                        current_user, r
                    ),
                    hint = format('As an administrator run: GRANT %I TO %I;', r, current_user);
        end;
    end loop;
end
$mig$;
