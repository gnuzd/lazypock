-- API keys as a table, instead of one JSON array in `_settings`.
--
-- The array had three problems a table does not: every write was a read-modify-write of
-- the whole list, so two concurrent creates (or a create and a revoke) silently lost one
-- of them; every request carrying a key re-read and re-parsed the list; and expiry was
-- compared as text, which is only correct while both timestamps were written by the same
-- formatter.
--
-- Server-internal, like `_sessions`: no endpoint serves this table, the records API never
-- resolves it, and only the superuser-only settings routes touch it. Only the digest is
-- stored, so a dump does not hand out usable keys.

create table if not exists _api_keys (
    id         text        primary key,
    -- SHA-256, hex. The raw key exists only in the response that created it.
    hash       text        not null,
    expires_at timestamptz,
    revoked_at timestamptz,
    created    timestamptz not null default now()
);

-- Lookup is by digest, so it must be unique as well as indexed.
create unique index if not exists _api_keys_hash_idx on _api_keys (hash);

-- Move the keys an existing installation already has. The array held RFC 3339 strings,
-- which `::timestamptz` parses directly.
--
-- A malformed entry is migrated as "no expiry" rather than failing the whole upgrade: a
-- timestamp nobody can parse is a key whose expiry nobody can state, and refusing to boot
-- over one hand-edited field would be the wrong trade. The regex guard is deliberately not
-- `pg_input_is_valid`, which is Postgres 16 and up while this server supports 15.
insert into _api_keys (id, hash, expires_at, revoked_at, created)
select entry->>'id',
       entry->>'hash',
       case
           when coalesce(entry->>'expires_at', '') ~ '^\d{4}-\d{2}-\d{2}[T ]'
               then (entry->>'expires_at')::timestamptz
           else null
       end,
       case when entry->>'revoked' = 'true' then now() else null end,
       case
           when coalesce(entry->>'created_at', '') ~ '^\d{4}-\d{2}-\d{2}[T ]'
               then (entry->>'created_at')::timestamptz
           else now()
       end
from _settings s
cross join lateral jsonb_array_elements(s.value) as entry
where s.key = 'auth.api_keys'
  and jsonb_typeof(s.value) = 'array'
  and coalesce(entry->>'id', '') <> ''
  and coalesce(entry->>'hash', '') <> ''
on conflict (id) do nothing;

-- The blob goes rather than staying behind: two stores of one credential is how a revoked
-- key keeps working after somebody revokes it in the other one.
delete from _settings where key = 'auth.api_keys';
