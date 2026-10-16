-- 0002_superusers.sql
--
-- Superusers are deliberately separate from any auth collection (PLAN.md 12.3):
-- they are not records in a user table, they cannot be created through the
-- records API, and they bypass access rules. The only creators are the CLI and
-- an existing superuser.

create table if not exists _superusers (
    id            text        primary key,
    email         text        not null,
    password_hash text        not null,
    -- Bumped to invalidate every outstanding token for this account at once.
    token_key     text        not null default '',
    created       timestamptz not null default now(),
    updated       timestamptz not null default now()
);

-- Case-insensitive uniqueness: `Admin@Example.com` and `admin@example.com` are
-- the same account.
create unique index if not exists _superusers_email_idx on _superusers (lower(email));
