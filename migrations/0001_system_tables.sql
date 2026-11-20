-- 0001_system_tables.sql
--
-- The lazypock system schema. Tables are named with a leading underscore, in
-- the same style as the original implementation, and are deliberately
-- unqualified (no `public.` prefix): every statement resolves against the
-- connection's `search_path` so a tenant can live in its own schema or
-- database without changes here (see PLAN.md section 14).
--
-- `_migrations` is deliberately *not* created here: the migration runner owns
-- the ledger and creates it before applying anything (crates/db/src/migrate.rs).

-- Collections: one row per collection; each maps to a real Postgres table.
create table if not exists _collections (
    id         text        primary key,
    name       text        not null unique,
    type       text        not null default 'base'
                   check (type in ('base', 'auth', 'view')),
    -- Collection-type specific settings (auth options, view SQL, file
    -- constraints, realtime mode, ...).
    options    jsonb       not null default '{}'::jsonb,
    -- Access rules keyed by action: listRule, viewRule, createRule, updateRule,
    -- deleteRule, manageRule. Null/absent = superuser only, "" = public.
    rules      jsonb       not null default '{}'::jsonb,
    created    timestamptz not null default now(),
    updated    timestamptz not null default now()
);

-- Fields describe the columns of a collection's table.
create table if not exists _fields (
    id           text        primary key,
    collection_id text       not null references _collections (id) on delete cascade,
    name         text        not null,
    type         text        not null,
    required     boolean     not null default false,
    is_unique    boolean     not null default false,
    -- Type specific settings (min/max, select values, relation target,
    -- file constraints and presets, ...).
    options      jsonb       not null default '{}'::jsonb,
    position     integer     not null default 0,
    created      timestamptz not null default now(),
    updated      timestamptz not null default now(),
    unique (collection_id, name)
);

create index if not exists _fields_collection_idx on _fields (collection_id, position);

-- Application settings: SMTP, storage, CORS, rate limits, signing keys, ...
create table if not exists _settings (
    key     text        primary key,
    value   jsonb       not null default '{}'::jsonb,
    updated timestamptz not null default now()
);

-- Persisted cron jobs.
create table if not exists _crons (
    id           text        primary key,
    name         text        not null,
    expression   text        not null,
    timezone     text        not null default 'UTC',
    enabled      boolean     not null default true,
    action       text        not null default 'sql'
                     check (action in ('sql', 'http', 'hook')),
    payload      jsonb       not null default '{}'::jsonb,
    last_run_at  timestamptz,
    last_status  text,
    next_run_at  timestamptz,
    created      timestamptz not null default now(),
    updated      timestamptz not null default now()
);

create index if not exists _crons_enabled_idx on _crons (enabled, next_run_at);

-- Refresh sessions. Only a hash of the refresh token is stored.
create table if not exists _sessions (
    id            text        primary key,
    collection_id text,
    user_id       text,
    token_hash    text        not null,
    -- Rotation family: all tokens descended from one login share this id, so a
    -- reused (stolen) refresh token lets us revoke the whole family.
    family_id     text        not null,
    expires_at    timestamptz not null,
    revoked_at    timestamptz,
    user_agent    text,
    ip            text,
    created       timestamptz not null default now()
);

create index if not exists _sessions_family_idx on _sessions (family_id);
create index if not exists _sessions_expiry_idx on _sessions (expires_at);
create unique index if not exists _sessions_token_hash_idx on _sessions (token_hash);

-- Background job queue (FOR UPDATE SKIP LOCKED).
create table if not exists _jobs (
    id            text        primary key,
    kind          text        not null,
    payload       jsonb       not null default '{}'::jsonb,
    status        text        not null default 'pending'
                      check (status in ('pending', 'running', 'done', 'failed', 'dead')),
    priority      integer     not null default 0,
    attempts      integer     not null default 0,
    max_attempts  integer     not null default 5,
    run_at        timestamptz not null default now(),
    locked_by     text,
    locked_at     timestamptz,
    last_error    text,
    created       timestamptz not null default now(),
    updated       timestamptz not null default now()
);

create index if not exists _jobs_ready_idx on _jobs (status, run_at, priority desc);

-- Object deletions queued in the same transaction as the row change, so an
-- object is only removed after the transaction commits.
create table if not exists _file_outbox (
    id          text        primary key,
    storage_key text        not null,
    attempts    integer     not null default 0,
    run_at      timestamptz not null default now(),
    last_error  text,
    created     timestamptz not null default now()
);

create index if not exists _file_outbox_ready_idx on _file_outbox (run_at);

-- Named image resize presets, editable from the Studio.
create table if not exists _image_presets (
    name       text        primary key,
    width      integer,
    height     integer,
    fit        text        not null default 'cover'
                   check (fit in ('cover', 'contain', 'fill', 'inside', 'outside')),
    format     text        not null default 'webp'
                   check (format in ('webp', 'avif', 'jpeg', 'png')),
    quality    integer     not null default 80 check (quality between 1 and 100),
    created    timestamptz not null default now(),
    updated    timestamptz not null default now()
);

-- Short-term realtime replay log for `Last-Event-ID` resume.
create table if not exists _rt_log (
    id         bigserial   primary key,
    collection text        not null,
    op         text        not null check (op in ('create', 'update', 'delete')),
    record_id  text        not null,
    origin     text,
    created    timestamptz not null default now()
);

create index if not exists _rt_log_created_idx on _rt_log (created);

-- Request logs.
create table if not exists _logs (
    id           bigserial   primary key,
    method       text,
    path         text,
    status       integer,
    duration_ms  double precision,
    user_id      text,
    ip           text,
    user_agent   text,
    error        text,
    created      timestamptz not null default now()
);

create index if not exists _logs_created_idx on _logs (created desc);

-- Admin action audit log.
create table if not exists _audit (
    id         bigserial   primary key,
    actor      text,
    action     text        not null,
    target     text,
    details    jsonb       not null default '{}'::jsonb,
    created    timestamptz not null default now()
);

create index if not exists _audit_created_idx on _audit (created desc);
