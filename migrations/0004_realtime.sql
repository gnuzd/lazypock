-- 0004_realtime.sql
--
-- The change-event trigger every collection table gets (PLAN.md section 8.2).
--
-- One shared trigger function, installed per collection by the DDL engine, so
-- `collections::create` is what makes a table publish changes. It does two
-- things:
--
--   * appends to `_rt_log`, which is what lets a reconnecting client resume with
--     `Last-Event-ID` instead of refetching;
--   * `pg_notify`s a small payload — collection, operation, record id and the
--     originating connection — on the channel every node listens on.
--
-- The payload deliberately carries no record data: `NOTIFY` payloads are capped
-- at 8000 bytes, so a node fetches what it needs when it needs it.

create or replace function lazypock_change_event() returns trigger
    language plpgsql
    as $$
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
    insert into _rt_log (collection, op, record_id, origin)
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
$$;

comment on function lazypock_change_event() is
    'Emits a realtime change event on lazypock_rt and records it in _rt_log.';

-- The trigger runs with the invoking user's privileges, so the restricted roles
-- need to append to the log. They are granted it by migration 0003's blanket
-- grant, but `_rt_log` is listed here too so the requirement is explicit.
do $$
declare
    target_schema text := current_schema();
begin
    execute format(
        'grant insert on %I._rt_log to lazypock_anon, lazypock_authenticated',
        target_schema
    );
end
$$;
