-- One-time tokens for the email flows: verification, password reset and email
-- change (PLAN.md 7.1).
--
-- Each flow mints a token here and emails it; the confirm half looks the token up
-- and applies the change. The row is deleted when the token is used, so a token is
-- single-use as well as short-lived (24 hours).
--
-- `password_hash` holds the **SHA-256 of the opaque token**, not a password KDF.
-- The token is 32 random bytes — not a user-chosen secret — so there is nothing to
-- brute-force, and the confirm path has to be one indexed read rather than a
-- verification against every outstanding row. The reference did the latter, with
-- bcrypt; this keeps the same behaviour without the O(n) slow hashes.
--
-- `sent_to` is the address the token was mailed to. For an email change it is the
-- *new* address, which is what makes the destination unforgeable: the confirm
-- endpoint reads it from here rather than trusting the caller.
--
-- `_otps` holds login-adjacent material, so row-level security is enabled in this
-- migration, with no policy: default-deny. The server's own connection is the
-- schema owner and bypasses it (see the note in 0003_rls.sql on why FORCE is
-- deliberately absent), while a request running as lazypock_anon or
-- lazypock_authenticated can read none of it.

create table if not exists _otps (
    id             text        primary key,
    collection_ref text        not null,
    record_ref     text        not null,
    password_hash  text        not null,
    sent_to        text,
    created_at     timestamptz not null default now(),
    updated_at     timestamptz not null default now()
);

comment on table _otps is
    'Single-use email tokens (verification, password reset, email change). Credential-adjacent.';

comment on column _otps.password_hash is
    'SHA-256 of the opaque token, hex. A KDF is unnecessary for 256 bits of randomness.';

-- The confirm path looks a token up by its hash, scoped to the collection.
create unique index if not exists _otps_token_idx
    on _otps (collection_ref, password_hash);

comment on index _otps_token_idx is
    'The confirm lookup: one indexed read per token, not a scan of every outstanding row.';

create index if not exists _otps_record_idx
    on _otps (collection_ref, record_ref);

alter table _otps enable row level security;
