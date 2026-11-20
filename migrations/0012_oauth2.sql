-- OAuth2 external auth: the two link tables the flow needs.
--
-- `_external_auths` maps a provider identity to a record: (collection, provider,
-- provider_id) -> user_id. It is what makes a second sign-in with the same
-- provider land on the same account instead of creating another one, and the
-- unique index is what makes the find-or-create race-safe — two callbacks racing
-- to link the same identity insert one row and the loser reads it.
--
-- `_auth_origins` records the app origin an OAuth2 flow started from, keyed by the
-- flow's `state`. The provider redirects the popup back to this server, which has
-- no other way to know which origin may receive the result; without it the popup
-- bridge would have to trust a `postMessage` target from the request, which is
-- exactly what an attacker would set.
--
-- Both tables are credential-adjacent: an external-auth row is a login that
-- bypasses the password, and an origin is a redirect target. Row-level security is
-- enabled **here, with the tables**, and there is no policy — which is default-deny.
-- The server's own connection is the schema owner and bypasses it (see the note in
-- 0003_rls.sql on why FORCE is deliberately absent), while a request running as
-- `lazypock_anon`/`lazypock_authenticated` can read neither.

create table if not exists _external_auths (
    id          text        primary key,
    collection  text        not null,
    provider    text        not null,
    provider_id text        not null,
    user_id     text        not null,
    created_at  timestamptz not null default now(),
    updated_at  timestamptz not null default now()
);

comment on table _external_auths is
    'OAuth2 links: (collection, provider, provider_id) identifies a record. Credential-adjacent.';

create unique index if not exists _external_auths_identity_idx
    on _external_auths (collection, provider, provider_id);

create index if not exists _external_auths_user_idx
    on _external_auths (user_id);

create table if not exists _auth_origins (
    id             text        primary key,
    collection_ref text        not null,
    record_ref     text,
    fingerprint    text        not null,
    -- The pending flow: provider, PKCE verifier and the app origin to post back
    -- to. Kept in the table rather than in memory so the authorize request and
    -- the provider callback may land on different nodes.
    data           jsonb       not null default '{}'::jsonb,
    created_at     timestamptz not null default now(),
    updated_at     timestamptz not null default now()
);

comment on table _auth_origins is
    'The app origin an OAuth2 flow started from, keyed by its state (fingerprint). Credential-adjacent.';

create unique index if not exists _auth_origins_fingerprint_idx
    on _auth_origins (collection_ref, fingerprint);

alter table _external_auths enable row level security;
alter table _auth_origins enable row level security;
