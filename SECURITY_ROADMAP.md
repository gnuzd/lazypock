# Security roadmap

A prioritized, **code-verified** list of the security and correctness gaps in the file
layer and the boot path, plus the capability work that follows them.

This file supersedes the earlier review pass. Every finding below was re-checked against
the tree at `main` @ `0ecd3eb` (0.23.1) and cites the lines it was checked against. Where
that earlier review was wrong or too narrow, the correction is recorded in
[Corrections](#corrections) rather than quietly dropped.

Scope: read the code, plus the documented claims next to it. Nothing here was
benchmarked or exploited. Items marked *unverified* are the ones that need a live
environment or a human decision, not more reading.

## How to read this

- **P0** — a defect that loses or exposes data today. Fix before anything else.
- **P1** — blocks real use (a promise the code does not keep).
- **P2** — capability work.
- **P3** — when there is demand.
- **Decided** — a choice made here, with the reasoning, so it is not re-litigated.

Findings keep stable `F` numbers; the P0 list refers to them.

## Findings

| # | Finding | Severity | Evidence |
| --- | --- | --- | --- |
| F1 | A record write can claim **any** file id: `_files` is re-pointed at the caller's collection/record/field with no ownership filter, so anyone who may write a record can take a file that belongs to somebody else and then delete it (its owner's record is left pointing at a missing object). | **High** — cross-tenant destroy-by-id | `crates/files/src/store.rs:535-561`, called from `crates/api/src/records.rs:453-461` |
| F2 | `DELETE /api/files/{collection}/{record}/{filename}` needs only *an authenticated request*. It also bypasses `_files` entirely: the object goes, the row stays `ready`, the owning record still references a file that now 404s. | **High** — unauthorized delete + row/object desync | `crates/api/src/files.rs:1055-1089`, route `crates/api/src/router.rs:91-93` |
| F3 | The batch endpoint never calls `sync_files`, so a file field written through `/api/batch` is stored but never attached, and its editor references are never recorded — the reaper then reclaims the "unattached" upload. | **Medium** — silent data loss | `crates/api/src/batch.rs:237-262` vs `crates/api/src/records.rs:269,336` |
| F4 | If the connection cannot switch to `lazypock_anon`/`lazypock_authenticated`, boot logs a warning and serves on. That is not a degraded mode: superusers use `RequestContext::owner()` (no `SET ROLE`) so the Studio keeps working while **every** anonymous and authenticated request fails in `apply()`. | **High** — RLS silently absent, end users 100% broken | `crates/app/src/main.rs:253-265`, `crates/api/src/auth.rs:97-108`, `crates/db/src/rls/context.rs:236-242` |
| F5 | The token signing key is stored as plaintext base64 DER in `_settings.auth.signing_key`. Read-only DB access (a dump, a replica, a backup) plus that value mints a token for any user — including superusers. | **High** — DB read escalates to app takeover | `crates/auth/src/service.rs:433-478` |
| F6 | `delete_by_id`'s doc comment claims "a caller who is neither a superuser nor the uploader may not delete somebody else's file". No `uploaded_by` check exists; only `deleteRule`. | Low — doc/code mismatch | `crates/api/src/files.rs:858-864` vs `:888-921` |
| F7 | **Fixed for read/list and delete (F7a).** File rules *were* enforced by hand in `crates/api/src/files.rs`, and `_files`/`_file_refs` had no row-level security. They now do (`migrations/0015_file_rls.sql`), and the list/delete policies are generated from `files.rules` by `lazypock_files::policy` at boot and on every settings write. Serving remains a capability URL (F10). What is still application-level is the **write/attach** side: an attach policy would have to reach into the target record, and `rls::policy` refuses subqueries (F7b). | Medium → structural | `migrations/0015_file_rls.sql`, `crates/files/src/policy.rs` |
| F8 | ~~`POST /api/settings/storage/test` is documented as a full upload round-trip.~~ **Retracted** — the docs source says "probes the backend", which is what it does. What survives is part of F9: the probe is a `GET ?list-type=2&max-keys=1`, so it proves the credentials and the bucket and nothing about writing, reading back or deleting. The claim I recorded came from stale generated output under `docs/.svelte-kit/` (untracked and gitignored), not from the page source. | — retracted (see F9) | `docs/src/routes/files/+page.md:213-215` (accurate), `crates/api/src/settings.rs:518-524` |
| F9 | S3/R2 has only offline SigV4 assertions (timestamp, canonical query, signature shape); no PUT/GET/DELETE/LIST/presigned-PUT has ever run against a bucket, in CI or out. | Medium — unverified | `crates/files/src/s3.rs:833-1027` |
| F10 | Serving a file (`GET /api/files/{id}`, `?preset=`, `?thumb=`, and the key-addressed path) takes no `Principal` and consults no rule: a file id is a never-expiring capability URL. **This is documented in all three places** — `files/src/rules.rs`, the Studio's File rules modal, and the docs page ("a URL behaves as a capability… which is why the access rules cover listing and deleting rather than reading"). What is open is the *decision*: PocketBase moved to short-lived file tokens, and this repo should either keep the capability model deliberately or gate reads on the record's `viewRule`. | Medium — an open design decision, not an undocumented one | `crates/api/src/files.rs:978-1040`, `crates/files/src/rules.rs:24-27`, `docs/src/routes/files/+page.md:93-96` |
| F11 | There was no environment-driven superuser at all: `superuser create` needed `--password` or `LAZYPOCK_SUPERUSER_PASSWORD`, `--email` had no env counterpart, and the server docs claimed the CLI "prompts for a password" — it errors. | Low — bootstrap footgun + false doc claim | `crates/app/src/cli.rs:40-48`, `docs/src/routes/server/+page.svelte:225-240` |
| F12 | `_mfas` is synthesised as a Studio system collection with no table behind it, so it cannot hold a row. | Low — advertised capability that cannot function | `crates/api/src/collections.rs:344-380` |
| F13 | `SECURITY.md` links `TEST_COVERAGE_AUDIT.md` and `HOOKS_SECURITY.md` (neither exists) and `README.md#what-the-rule-engine-tests-cover` (no such anchor); its supported-versions table says 0.12.x. | Low — stale docs | `SECURITY.md:5-40` |

## Audit — the other hand-enforced surfaces

Read-only pass over the surfaces that are not backed by row-level security, done after the
file layer turned out to hide two cross-tenant holes: sessions and refresh rotation, API
keys, rate limiting, OAuth2/OTP flows, backup and settings import/export, the SQL console,
realtime delivery, hook execution, and Studio asset serving. Nothing here was exploited;
every finding cites the line it was read at.

Findings are `S`-numbered so they do not collide with the file-layer `F` numbers.

| # | Finding | Severity | Evidence |
| --- | --- | --- | --- |
| S1 | Refresh rotation is a read-then-write, not a conditional update: two simultaneous uses of one token both see `revoked = false`, both mint a token, and the replay that reuse detection exists to catch goes unnoticed. `consume` in the OTP flow does the same job correctly (`delete … returning`), which is the shape this should take. | Medium — the theft-detection guarantee can be raced away by a thief who is willing to race the victim | `crates/auth/src/sessions.rs:118-172` vs `crates/api/src/emails.rs:159-172` |
| S2 | `auth-refresh` rotates **before** it checks that the token belongs to the collection in the path, so a refresh token presented to the wrong collection's endpoint is consumed and the caller gets a 401 — the old token is dead, the new one is discarded, and the user is silently logged out. Related: a superuser refresh token carries an empty collection id, so that check is skipped entirely and it is accepted on any collection's path. | Medium — a client bug or a stale SDK logs people out; the superuser case is a routing oddity rather than a privilege change | `crates/api/src/auth.rs:411-436`, `:159-166` |
| S3 | API keys are one JSON array in `_settings` written read-modify-write with no lock or version: two concurrent creates, or a create and a revoke, silently lose one of the two. `verify` also re-reads and re-parses the whole array on every request that carries a key. | Medium — a revocation can be lost, which is the wrong direction for a credential | `crates/api/src/api_keys.rs:88-160`, `:131-152` |
| S4 | API-key expiry is compared as **text**: `expires_at > now` on two RFC 3339 strings. That only holds while both sides were written by the same formatter; a value with different precision (`.000` vs none) or a numeric offset compares backwards and keeps an expired key alive. | Low-Medium — needs a non-server-written value, and the admin settings path is the way one appears | `crates/api/src/api_keys.rs:143-150` |
| S5 | `SECURITY.md` says realtime is "authorized once, at join time … no per-record rule evaluation on delivery", and the code does the opposite: every event runs the collection's select policy for that subscriber's context, and deletes are gated on the record having been delivered before. **The code is stronger than the doc**, so this is a documentation defect that would mislead an auditor in the safe direction — still worth fixing, because a scope note that is wrong is a scope note nobody can rely on. | Medium — the security policy misstates a boundary | `SECURITY.md` (Scope notes) vs `crates/realtime/src/hub.rs:220-260`, `:336-370` |
| S6 | Nothing bounds realtime subscriptions per connection or connections per address: `subscribe=*` expands to every collection, and each subscription holds a queue and a `seen` buffer. Anonymous callers can do this. | Low — memory/CPU amplification, no data exposure | `crates/api/src/realtime.rs:41-95`, `crates/realtime/src/hub.rs:141-171` |
| S7 | `GET …/auth-methods` is public and writes: one upsert per configured provider, plus a sweep `DELETE`, on every call. Bounded by the flow TTL and the per-address rate limit, so it cannot grow without bound — but a read endpoint that writes is worth knowing about. | Low | `crates/api/src/auth.rs:471-520`, `crates/api/src/oauth2.rs:483-515` |
| S8 | A presented API key that is invalid, revoked or expired becomes an **anonymous** principal rather than a 401, so a client with a typo'd or revoked key silently gets public access instead of a clear refusal. | Low — fail-open to the least privilege, but it hides a real misconfiguration | `crates/api/src/auth.rs:196-201` |
| S9 | `Hub::subscribe`/`unsubscribe` use `if let Ok(guard) = self.subscribers.lock()`: a poisoned lock silently drops the subscription and the client keeps an open SSE stream that will never deliver anything. | Low — availability, and a silent one | `crates/realtime/src/hub.rs:152-183` |
| S10 | The SQL console can read any table, including `_settings` (ciphertext for the storage secret and the signing key) and `_sessions` (token hashes). Superuser-only, and the same trust as the settings export, but it is worth stating rather than leaving to be discovered. | Info | `crates/api/src/sql.rs:1-40` |
| S11 | Import caps the **compressed** upload at 512 MiB and has no uncompressed-size cap, so a small archive can expand into a large in-memory import. Superuser-only. | Low — self-inflicted DoS with an already-trusted token | `crates/api/src/backup.rs:30-34`, `crates/db/src/backup.rs` (zip read) |

### Fixed

S1, S2, S3, S4, S5 and S8 — one change, because they are all "a credential handled slightly
wrong":

- **S1** — rotation now claims the row with a conditional `UPDATE … where revoked_at is null
  and expires_at > now() returning …`, so only the request that flips the flag mints the next
  token. Consequence, stated rather than discovered: two *simultaneous* refreshes of one token
  revoke the family, exactly as a sequential replay always did, so a client must not refresh
  concurrently — which is also how the TypeScript SDK behaves.
- **S2** — the collection check runs before the rotation (`sessions::peek`), so a mis-targeted
  refresh is refused **without** being consumed, and a superuser session is bound to the
  `_superusers` path like any other session is bound to its collection.
- **S3 + S4** — API keys live in `_api_keys` (migration 0014) with a unique index on the
  digest: create and revoke are single statements, lookup no longer grows with the number of
  keys, and expiry is compared in SQL against `timestamptz`. Existing keys are migrated and the
  `_settings` blob deleted, so there is one store rather than two.
- **S5** — `SECURITY.md` now describes what the dispatcher does: rule evaluation per event,
  deletes gated on prior delivery. It says that is stronger than PocketBase, instead of
  claiming to match it.
- **S8** — a value carrying the key prefix *is* a key attempt, so wrong/revoked/expired is a
  **401** naming the credential, not a silent demotion to anonymous. Client-visible: it used to
  be a 403 from the endpoint's own check, and a revoked key no longer keeps working against
  public endpoints.

S6, S7, S9, S10 and S11 stay open: none of them exposes data.

### Verified sound in the same pass

Worth recording so the next audit does not repeat it:

- **Refresh tokens**: 256-bit CSPRNG value, only a SHA-256 digest stored, rotation, family
  revocation on replay, expiry, and a purge job (`auth/src/sessions.rs`). The race in S1 is
  the one gap.
- **Credential columns cannot leak through a record**: `password_hash` and `token_key` are
  absent from the map that both filters and `fields=*` are built from, so they can be
  neither filtered on nor returned (`db/src/query/compile.rs:76-95`, `db/src/records.rs:364`).
- **API keys at rest**: SHA-256 digest, constant-time comparison, `auth.`-prefixed so the
  settings read and export both redact them (`api/src/api_keys.rs:131-152`,
  `api/src/settings.rs:141-205`).
- **OTP flows**: 256-bit token, hashed at rest, single use through `delete … returning`,
  TTL-checked on read *and* delete, swept on mint, and an email change deliberately does not
  consume the token before the password is checked (`api/src/emails.rs:55-172`).
- **Admin surface**: one `require_superuser`, one `Superuser` extractor; API keys are
  refused everywhere a superuser is required, and the single exception
  (`require_superuser_or_api_key`) is limited to the collection reads codegen needs
  (`api/src/auth.rs:918-996`).
- **Settings export** redacts `auth.*` and every secret-named leaf, and reports what it
  redacted instead of dropping it silently (`api/src/settings.rs:141-275`).
- **SQL console**: leading-keyword check *and* a read-only transaction *and* a 10 s statement
  timeout *and* a row cap — the belt-and-braces answer to "one check is not enough"
  (`api/src/sql.rs:1-40`).
- **Hooks**: QuickJS with a dispatch deadline (2 s) and explicit limits, and no filesystem or
  network bindings at all (`hooks/src/runtime.rs:13-40`).
- **Studio**: assets are looked up in a map, not joined onto a path, so there is no traversal
  surface (`api/src/studio.rs`).
- **Realtime delivery**: per-event select-policy evaluation under the subscriber's own RLS
  context, with deletes gated on prior delivery (`realtime/src/hub.rs:220-260`). See S5 for
  the documentation that contradicts this.

## P0 — fix now

Order chosen so the two data-loss defects land together: F1 and F2 are the same
invariant seen from two directions (a file may only be attached or removed by someone
who owns it or may write the owning record).

- **1. F1 + F3 — file ownership at attach time.** `store::attach` gains an owner filter:
  a non-superuser may attach only a file they uploaded, and no file may be re-pointed
  from one record/field to another. `sync_files` takes the caller's `Principal`, and the
  batch endpoint calls it too. Regression tests: a second user cannot claim the first
  user's id, and a record delete cannot destroy a file it did not own.
- **2. F2 + F6 — one delete path.** The key-addressed route resolves the key to its
  `_files` row and delegates to `delete_by_id` (deleteRule, the 409-when-referenced
  guard, `?force`), so the row and the object go together. The route stays: the
  key-addressed `GET` stays, the Elixir-era server had the pair, and removing it is a
  silent break. The uploader sentence in `delete_by_id`'s comment is corrected to what
  the code does.
- **3. F4 — `LAZYPOCK_REQUIRE_RLS`.** On by default: boot fails when the probes cannot
  switch to the request roles. `LAZYPOCK_REQUIRE_RLS=off` restores the warning for the
  one deployment shape that is genuinely fine without them (superuser tokens only).
- **4. F5 — encrypt the signing key.** The AES-256-GCM cipher moves from
  `lazypock_files::Secrets` to `lazypock_core` so there is one implementation; the
  signing key is stored encrypted under `LAZYPOCK_SECRET_KEY` when that is set, a
  plaintext key is read for back-compat and rewritten encrypted on load, and a key that
  cannot be decrypted fails boot loudly rather than starting with a wrong key. Threat
  model, stated: this separates "a leaked dump" from "a minted token"; it does not
  protect against a compromise of the environment.
- **5. F11 — the bootstrap story.** `LAZYPOCK_SUPERUSER_EMAIL` for symmetry with
  `LAZYPOCK_SUPERUSER_PASSWORD`, and a boot-time path for a deployment with no browser
  and no shell: both variables set creates the first account **only while none exists**,
  under the same advisory lock as the Studio's Setup. There is deliberately **no default
  superuser and no default password** — a redeploy must never reset a password, and half
  of the pair is a boot error rather than a guess.

## P1 — blockers for real use

- **6. ~~Audit the other hand-enforced surfaces.~~ Done — see [the audit](#audit--the-other-hand-enforced-surfaces).** The file layer was hand-rolled and had
  two cross-tenant holes; everything else that is not RLS-backed has not been read with
  the same method: sessions and refresh rotation, API keys, rate limiting, OAuth2, backup
  and restore import, Studio endpoint gating, **realtime delivery** (authorised once at
  join, then every change is delivered unexamined — `SECURITY.md` states it), and hook
  files (full server privileges). Read-only pass; the two findings above paid for
  themselves twice.
- **7. Non-image uploads — decided: not now.** Per-field MIME allowlist, magic-byte
  validation and size limits already exist; the accepted *type* set is png/jpeg/gif/webp
  and is decided by `ImageFormat`. Widening it is not just a validator change: a
  non-image needs a safe serving policy (`attachment` disposition plus `nosniff`) and a
  decision about which magic-byte set to trust, so the image-only set is kept
  deliberately rather than shipping the unsafe half. Revisit on demand.
- **8. F10 — decide file-read authorization.** Either keep capability-URL semantics as the
  contract (a file id is unguessable and permanent) or gate reads on the owning record's
  `viewRule`. PocketBase's answer is a short-lived file token; the migration cost is real
  for every stored URL, so this is a decision rather than a bug fix. The model is already
  documented in all three places, so nothing here is a documentation debt.
- **9. ~~Fewer round trips per request~~ Done.** `apply()` now publishes the role switch and
the request claims in **one** statement: `set_config('role', $n, true)` is the parameterised
equivalent of `SET LOCAL ROLE`. `NULL` resets `role` to the session user, which is how the owner
path is expressed — `SET ROLE NONE` is special syntax and cannot be a parameter. A regression
test applies a restricted context and then an owner one in the same transaction, so a reset
that silently did nothing would fail. Verified: `cargo test --workspace`, 11/11 in
`crates/db/tests/rls.rs`.

- **6a. S1–S4, S5 and S8 — the audit's security findings.** Done: rotation is a
  compare-and-swap, the collection check happens before the rotation, API keys live in
  `_api_keys` with the expiry compared in SQL, and a wrong or revoked key is a 401 rather
  than a silent demotion to anonymous. The remaining `S` numbers stay open because none of
  them exposes data.

## P2 — capability expansion

- **10. Hook bindings**, in this order: `$http`, then `$app`, then `$db` (which needs its
  own design for transactions and RLS). Today `crates/hooks/src/bootstrap.js` registers
  only the six record before/after hooks, and an async handler *throws* — so this is
  bigger than "partial bindings".
- **11. MFA/TOTP**, with `_mfas` becoming a real table and getting RLS in the same
  migration (F12; its rows are credential-adjacent, and the Studio already lists a
  `_mfas` collection that cannot hold a row). Start with superusers, include recovery
  codes.
- **12. File rules as RLS policies.** Read/list and delete are done (F7a): `_files` and
  `_file_refs` have RLS, and the list/delete policies are generated from `files.rules`
  (the same parser and compiler a collection rule uses). Remaining: the attach policy,
  which needs subquery support in `rls::policy`; until then `store::attach`'s ownership
  filter is the write-side invariant (F7b).
- **13. PocketBase data importer**, only if someone needs to migrate.

## P3 — when there is demand

- **14. Outbound webhooks** (built on hooks once `$http` exists).
- **15. Fuzzing in CI**, targeting the filter parser and the import archive.

## Decided (with the reasoning, so it is not re-opened by accident)

- **The key-addressed delete route stays** and becomes a resolver. The TS SDK and the
  Studio delete by id (checked), so nothing breaks either way; keeping it costs one
  query and keeps Elixir-era clients working.
- **`deleteRule` alone decides file deletion.** No implicit "uploader may delete" beyond
  the rule: PocketBase's semantics are `deleteRule`, an absent rule means superusers,
  and a `Public` rule is then a deliberate choice by the operator. F6 is fixed by
  correcting the comment, not by adding a check that would make `Public` a lie.
- **RLS enforcement is the default, not a warning.** A server that cannot switch roles
  is not "running without RLS", it is answering 500 to every end-user request.
- **No default superuser, ever.** A baked-in password is the single most exploited
  thing in this class of product. What exists instead: first-run Setup in the Studio,
  the CLI, and `LAZYPOCK_SUPERUSER_EMAIL` + `LAZYPOCK_SUPERUSER_PASSWORD` at boot for a
  container that has neither a browser nor a shell yet. All three are gated on "no
  superuser exists" and serialised on one advisory lock, so a redeploy cannot reset a
  password and two of them cannot race.
- **Upload is authenticated, attach is authorised.** An upload that names a record does
  not check that the caller may write it, and does not need to: a file cannot reach a
  record except through a write to that record, which its rules already gate. The
  uploader check at attach time is what stops a file id being claimed, and the docs now
  say so rather than leaving it to be inferred from a 403. The residual — burning
  another tenant's storage by uploading into their prefix — is a quota question, not a
  data one, and there is no per-account quota to spend against it yet.

## Corrections

Corrections to the earlier review pass (the pre-verification draft of this roadmap):

| Claim | Correction |
| --- | --- |
| "Legacy file delete has no authorization beyond *is authenticated*" | Correct, and worse: it also leaves the `_files` row in place (F2). |
| "Upload checks only that the record exists, not that the caller may update it" | Correct as far as it goes, but the sharper hole is at **attach** time, which is where a file changes owner (F1). An upload-time record check would not close it. |
| "If the connection cannot switch to the RLS roles, the server only logs a warning" | Correct, and the consequence is stronger than "RLS is not in force": every anonymous and authenticated request fails (F4). |
| "The signing key is stored in plaintext unless `LAZYPOCK_SIGNING_KEY` is set" | Correct. The draft's recommendation ("document that production must set the env var") was too weak: the value is a token-minting capability for any user, so it is encrypted at rest instead. |
| "S3/R2 has never been exercised against a live bucket" | Correct. The draft missed that `POST /api/settings/storage/test` exists, and I then recorded that its docs claimed a full round-trip (F8). **That was my error**: the claim exists only in stale generated output under `docs/.svelte-kit/`, which is untracked and gitignored — never grep a build directory and treat it as source. The page source says "probes the backend", and the endpoint is a list probe. |
| "The docs page lists file serving as 'public' and says nothing about the consequence" (F10) | Wrong for the same reason, and corrected the same way: `docs/src/routes/files/+page.md:93-96` states the capability model and why the rules cover listing and deleting rather than reading. Same stale-output mistake, second time. |
| "Since PR #179: Postgres TLS, `LAZYPOCK_SIGNING_KEY`, a redesigned file layer" | Imprecise: `53d79269` (signing key), `d0a08257` (`_files`) and `18c1cbd1` (encrypted storage secrets) are **ancestors of the #179 merge** — they arrived with the rewrite. Only the dedicated-LISTEN TLS work (#210) is since. |
| "Ahead of PocketBase: authorization enforced by database RLS rather than in the application layer" | True for collection tables (policies generated in `crates/db/src/ddl.rs`). Not true for the file layer, which is hand-enforced and RLS-free (F7) — which is exactly where the P0 findings are. |
| "Hooks are partial: no `$db`, `$http`, `$app`" | Understated: only the six record hooks are registered, and a promise-returning handler throws. |
| "755 tests" | 758 `#[test]`/`#[tokio::test]` attributes at `0ecd3eb`; the README badge says 755. |

## Status

| Item | State |
| --- | --- |
| F1 + F3 ownership at attach | done — regression tests in `crates/api/tests/files.rs` |
| F2 + F6 one delete path | done — regression test in `crates/api/tests/files.rs` |
| F4 `LAZYPOCK_REQUIRE_RLS` | done |
| F5 signing key at rest | done |
| F11 bootstrap env + docs | done — boot, CLI, docs, compose example; verified end to end against a scratch database |
| F13 stale `SECURITY.md` links | done |
| Upload/attach semantics + the key-addressed delete, in the docs | done — `docs/src/routes/files/+page.md` (uploading, API table) and the production notes on the server page |
| F8 + F9 bucket round-trip + MinIO CI | **F8 retracted** (the docs are accurate; see Corrections). F9 open: no write/read/delete has run against a bucket — the owner is testing R2 by hand, so this stays a note rather than a planned item |
| F10 file-read decision | documented everywhere it needs to be; the **decision** (capability URLs vs PocketBase-style file tokens, or `viewRule` gating) is open |
| F7a RLS on `_files` | done — `_files`/`_file_refs` have RLS, and list/delete policies are generated from `files.rules` at boot and on every settings write. Serving stays capability-based (F10). Tests: `crates/files/src/policy.rs`, `crates/api/tests/files.rs` |
| F7b attach policy | open — needs subquery support in `rls::policy`; `store::attach`'s ownership filter is the write-side invariant until then |
| P1 #9 one round trip per request | done — `rls::context::apply` sets role + claims in one statement, with a same-transaction owner-reset regression test |
