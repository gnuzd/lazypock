-- 0018_realtime_hardening.sql
--
-- The change-event trigger used to run with the **invoking** role's privileges.
-- That is why migration 0004 had to grant `lazypock_anon` and
-- `lazypock_authenticated` INSERT on `_rt_log`, and it left a capability the
-- restricted roles never needed: any path that can run SQL as one of them could
-- append rows to the change log, and the change log is what a resuming client is
-- replayed from.
--
-- Making the function SECURITY DEFINER removes the need for the grant, so the grant
-- goes. The `search_path` is pinned because a SECURITY DEFINER function that
-- resolves an unqualified name is a privilege-escalation hazard: a temp table or a
-- schema earlier in the path could shadow what it touches. `pg_catalog` is placed
-- first so no built-in can be shadowed either, and the one relation the function
-- writes is schema-qualified below.
--
-- This does not widen what a write can do: the AFTER trigger runs once the row's
-- own INSERT/UPDATE/DELETE policy has already been satisfied, and the function
-- only appends to the log and notifies.

do $mig$
declare
    target_schema text := current_schema();
begin
    execute format($q$
        create or replace function lazypock_change_event() returns trigger
            language plpgsql
            security definer
            set search_path = pg_catalog, pg_temp
            as $body$
        declare
            change_id bigint;
            operation text;
            target_id  text;
        begin
            operation := case tg_op
                when 'INSERT' then 'create'
                when 'UPDATE' then 'update'
                else 'delete'
            end;

            target_id := case when tg_op = 'DELETE' then old.id::text else new.id::text end;

            -- `nullif` keeps a request without a connection id indistinguishable from
            -- one that never had one: both mean "every subscriber sees this".
            insert into %I._rt_log (collection, op, record_id, origin)
            values (
                tg_table_name,
                operation,
                target_id,
                nullif(current_setting('request.connection_id', true), '')
            )
            returning id into change_id;

            perform pg_notify(
                'lazypock_rt',
                jsonb_build_object(
                    'i', change_id,
                    'c', tg_table_name,
                    'op', operation,
                    'id', target_id,
                    'o', nullif(current_setting('request.connection_id', true), '')
                )::text
            );

            return null;
        end
        $body$;
    $q$, target_schema);
end
$mig$;

comment on function lazypock_change_event() is
    'Emits a realtime change event on lazypock_rt and records it in _rt_log. '
    'SECURITY DEFINER, so the restricted roles do not need write access to the log.';

-- The function writes the log as its owner; the restricted roles do not. Reading is
-- revoked too: the log names collections and record ids, and no user-facing path
-- needs it.
do $mig$
declare
    target_schema text := current_schema();
begin
    execute format(
        'revoke insert, select, update, delete on %I._rt_log '
        'from lazypock_anon, lazypock_authenticated',
        target_schema
    );
    execute format(
        'revoke usage, select on sequence %I._rt_log_id_seq '
        'from lazypock_anon, lazypock_authenticated',
        target_schema
    );
end
$mig$;