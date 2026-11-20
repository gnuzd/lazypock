-- The fields an auth collection has whether or not anybody declared them.
--
-- The *columns* have existed since the table was created, but they were not recorded
-- as fields — and a client reads a collection's **definition**, so an auth collection
-- showed only the fields a user had added. An installation's `users` collection
-- therefore listed `id, name, avatar`, with no sign that accounts had an email or a
-- password at all.
--
-- Two kinds of entry, following PocketBase:
--
--   * `email`, `password_hash`, `token_key` — the account's credentials. `hidden`
--     keeps them out of client responses and `writeOnly` marks the password as
--     something you set and never read back. The API also excludes them from the map
--     it builds filters and projections from, which is what `fields=*` returns.
--   * `emailVisibility`, `verified` — the account's state.
--
-- `emailVisibility` did not exist as a column before this migration, so it is added
-- here. Idempotent throughout: a re-run adds nothing, and an auth collection that
-- already has one of these fields keeps its own version.

-- A 15-character id, the same length the application generates.
create or replace function _lazypock_field_id() returns text language sql volatile as $$
    select substr(md5(random()::text || clock_timestamp()::text), 1, 15)
$$;

do $$
declare
    target record;
begin
    for target in select id, name from _collections where type = 'auth' loop
        execute format(
            'alter table %I add column if not exists "emailVisibility" boolean not null default false',
            target.name
        );

        insert into _fields (id, collection_id, name, type, required, is_unique, options, position)
        values
            (_lazypock_field_id(), target.id, 'email',           'email',    true,  true,  '{}'::jsonb, 1),
            (_lazypock_field_id(), target.id, 'password_hash',   'password', false, false,
                '{"hidden": true, "writeOnly": true}'::jsonb, 2),
            (_lazypock_field_id(), target.id, 'token_key',       'text',     false, false,
                '{"hidden": true}'::jsonb, 3),
            (_lazypock_field_id(), target.id, 'emailVisibility', 'bool',     false, false, '{}'::jsonb, 4),
            (_lazypock_field_id(), target.id, 'verified',        'bool',     false, false,
                '{"hidden": true}'::jsonb, 5)
        on conflict (collection_id, name) do nothing;
    end loop;
end $$;

drop function _lazypock_field_id();
