# PLAN: lazypock — a PocketBase-style BaaS in Rust + Postgres

> Project name: **lazypock** · License: **MIT** · Branch: `feat/rust-rewrite`
>
> Top priorities: **small binary, low RAM, lowest possible API latency**; many instances on a
> small VPS (2 vCPU / 2 GB); horizontal scaling; solid realtime.

---

## 0. Context: this is a rewrite, not a greenfield build

`lazypock` already existed as an **Elixir/Phoenix** implementation (v0.22.1). It has been
removed on this branch so the Rust rewrite starts from a clean tree.

- **No users.** There is no production data and no published contract to preserve. No data
  migration, no back-compat shims, no dual-running.
- The Elixir implementation is **kept in git history** as a design reference while building:
  - `git grep <pattern> main -- core`
  - `git show main:core/lib/lazypock/rules/enforcer.ex`
- **Kept on disk as reference** (to be rewritten/reused later, not part of the new build yet):
  - `studio/` — SvelteKit admin SPA (19 routes, ~40 components, TipTap, ERD, media library).
  - `lazypock-ts/` — the TypeScript SDK (separate repo, published to npm as `lazypock`).
  - `docs/` — the docs site.
- **Legacy design notes** moved to `docs/legacy/` (`BACKUP_SCALING_PLAN.md`,
  `HOOKS_SECURITY.md`, `STUDIO_AUDIT.md`, `TEST_COVERAGE_AUDIT.md`, `PRODUCTION_PLAN.md`).

Reference baseline of what was replaced (for comparison only, not a target):

| Metric | Elixir/Phoenix (v0.22.1) |
|---|---|
| Implementation | 25,343 LOC in `core/lib`, 977 tests across 69 files |
| Release binary | 22 MB (Linux x86_64), 22 MB (aarch64), 16 MB (macOS arm64) |
| Single binary | Burrito release, ImageMagick bundled |
| Realtime | Phoenix Channels (WebSocket), full-record payloads |
| Auth | HMAC `Phoenix.Token` (not JWT) |
| Rules | App-level SQL `WHERE` injection (no Postgres RLS) |
| Hooks | Elixir modules, hot-loaded (~80 hook points) |

The `lazypock` (npm) and GitHub repo names, and the MIT license, are preserved.

---

## 1. Decisions

| Area | Decision |
|---|---|
| Name / license | **lazypock** / **MIT** |
| Backend language | **Rust** (rewrite from scratch) |
| Database | Postgres (self-hosted first-class; Neon supported) |
| System tables | **`_collections`, `_fields`, `_settings`, `_crons`** (keep the existing `_`-prefixed names; **no `_sys` schema**) |
| Permissions | **Postgres RLS** (Studio rule editor generates policies) |
| Hooks | **TypeScript, auto-loaded, no recompile** (QuickJS) |
| Studio | SvelteKit (**bun**), `adapter-static`, **embedded in the binary** |
| Image scaling | **Separate service** (imgproxy), outside the main server |
| File storage | S3-compatible (Cloudflare R2) + local adapter; CDN optional |
| Postgres driver | **`tokio-postgres` + `deadpool-postgres`** (no sqlx) |
| Multi-tenant | **One database per tenant** (+ one process per tenant), section 14 |
| Realtime transport | **SSE** at `/api/realtime` (PocketBase-canonical); WebSocket optional later |
| Realtime source | **Postgres triggers + LISTEN/NOTIFY**, behind an `EventBus` trait; Redis backend **deferred** (section 8.8) |
| Realtime echo | **Connection-id filtering** — the originating device does not receive its own event (intentional divergence from PocketBase, section 8.7) |
| Public API | **PocketBase-canonical paths** (`/api/collections/{c}/records`, `/api/realtime`, `/api/files/...`) — section 5 |
| Tokens | **JWT (EdDSA)** access token + refresh sessions in the DB (section 7.2) |
| SDK | Rebuilt from scratch as `lazypock-ts` in its own repo (section 3) |
| Panic strategy | `unwind` — in-process recovery and a clean SSE drain beat the few hundred KB `abort` would save (section 18.6) |
| Versioning | Rust line starts at `0.1.0`; repo and npm names preserved |

### Decisions taken from the review (confirm or correct)

These were chosen because there are no users and because they match PocketBase, the product's
stated reference. They are cheap to change now and expensive later:

1. **PocketBase-canonical routes.** The Elixir version called records at `/api/:collection`,
   which broke its own "PocketBase-compatible" claim. The rewrite uses the real PocketBase
   shape so upstream SDKs/tools work. **Confirmed.**
2. **SSE, not Phoenix Channels.** Rebuild the SDK against SSE. **Confirmed.**
3. **Postgres RLS from the start** rather than porting the app-level rule enforcer. With no
   data to migrate, the secure design is cheaper now than retrofitting later. **Confirmed.**
4. **`_`-prefixed table names, no `_sys` schema.** Matches the existing convention and keeps
   unqualified DDL simple. **Confirmed.**
5. **Redis is not a hard dependency.** See section 8.8 for the analysis and the seam that
   keeps the door open. **Confirmed: Postgres-first with an `EventBus` seam, Redis deferred.**
6. **EdDSA JWT + JWKS** for access tokens. **Confirmed.**
7. **`panic = "unwind"`.** **Confirmed.**
8. **Rust line starts at `0.1.0`**, repo and npm names preserved. **Confirmed.**

---

## 2. Overall architecture

```
                    ┌────────────── CDN (Cloudflare) ──────────────┐
                    │  caches image variants, Studio static assets │
                    └───────┬───────────────────────────┬──────────┘
                            │                           │
                  /api, /api/realtime               /img/* (resize)
                            │                           │
                  ┌─────────▼─────────┐        ┌────────▼────────┐
   Load balancer →│  Server nodes 1..N│        │  Image service  │
   (Caddy/Nginx)  │  (Rust, stateless)│        │  (imgproxy)     │
                  │  API+Auth+RT      │        └────────┬────────┘
                  │  +Hooks+Studio    │                 │ reads origin
                  └───┬──────────┬────┘                 ▼
                      │          │               ┌───────────┐
                      │          └──────────────►│ R2 / S3   │
                      ▼                          └───────────┘
               ┌─────────────┐
               │  Postgres   │  ← single source of truth
               │ (LISTEN/    │     (data, RLS, jobs, outbox, realtime)
               │  NOTIFY)    │
               └─────────────┘
```

Core principles:

1. **Stateless nodes.** All state lives in Postgres (or S3). Adding a node = starting a process.
2. **Postgres is the message bus** for realtime, the job queue and cron locks. No Redis by
   default (less RAM, less complexity) — but abstracted behind a trait (section 8.8).
3. **One binary** contains: API, Auth, Realtime, Hooks runtime, Studio (embedded), migrations, CLI.
4. **Image processing never runs in the main server** — it causes RAM/CPU spikes and can OOM
   the process that serves the API.

---

## 3. Tech stack & repository layout

### 3.1 Stack

| Layer | Choice |
|---|---|
| Async runtime | `tokio` (multi-thread, worker threads = vCPU count) |
| HTTP | `axum` + `tower` + `hyper` |
| Postgres | `tokio-postgres` + `deadpool-postgres`; in-house migration runner, type mapping, LISTEN loop |
| SQL builder | small in-house builder for filter/sort/expand (fully parameterized) |
| Serialization | `serde`, `serde_json` |
| JSON responses | **let Postgres build the JSON** (`json_agg`, `to_jsonb`) and stream the bytes |
| Auth crypto | `argon2`, `jsonwebtoken` (EdDSA/RS256), `oauth2`, `totp-rs`, `webauthn-rs` (later) |
| Studio embedding | `rust-embed` + pre-compressed brotli at build time |
| JS runtime for hooks | `rquickjs` (QuickJS) + `oxc` to strip TypeScript |
| File watcher | `notify` |
| Config | `clap` + env |
| Observability | `tracing`, `metrics` + Prometheus endpoint |
| Allocator | `mimalloc` (tuned to return memory to the OS) |
| Build profile | `opt-level=3`, `lto="fat"`, `codegen-units=1`, `strip=true` (see 18.6 re `panic`) |

### 3.2 Repo layout (Cargo workspace at the repo root)

```
lazypock/
├─ Cargo.toml                  # workspace (license = "MIT")
├─ LICENSE                     # MIT
├─ PLAN.md                     # this file
├─ README.md
├─ crates/
│  ├─ app/                     # main binary: main.rs, CLI (serve, migrate, superuser, export)
│  ├─ core/                    # config, errors, types, id generator, tracing
│  ├─ db/                      # pool, migrations runner, introspection, schema cache
│  ├─ query/                   # filter/sort/expand/fields parser → SQL builder
│  ├─ api/                     # axum router, CRUD handlers, validation, batch
│  ├─ auth/                    # users, sessions, JWT, OAuth2, OTP, MFA, external JWKS
│  ├─ rls/                     # policy generation, set_config claims, role management
│  ├─ realtime/                # SSE, subscription manager, fan-out, EventBus
│  ├─ files/                   # upload, validation, S3/R2 client, delete outbox
│  ├─ hooks/                   # QuickJS runtime, TS transpile, loader, JS API bindings
│  ├─ jobs/                    # queue (SKIP LOCKED), cron, leader lock
│  ├─ mailer/                  # SMTP, templates (verify, reset password, OTP)
│  ├─ admin/                   # Studio-only API (collections, settings, logs)
│  └─ studio-embed/            # build.rs: build SvelteKit → compress → rust-embed
├─ studio/                     # SvelteKit app (bun) — reference copy exists; rebuilt here
├─ services/
│  └─ image/                   # imgproxy config
├─ migrations/                 # system migrations (SQL, versioned)
├─ hooks/                      # example *.pb.ts
├─ deploy/
│  ├─ docker-compose.yml       # app + postgres + image service + caddy
│  └─ Dockerfile               # multi-stage, distroless
└─ docs/                       # docs site (existing) + docs/legacy/ (Elixir-era notes)
```

`lazypock-ts` stays a **separate repository** and is rebuilt there (section 3.3), keeping the
existing npm package name `lazypock`.

### 3.3 SDK

- `lazypock-ts` is rebuilt from scratch against the PocketBase-canonical API and SSE realtime.
- Its current contents remain in `lazypock-ts/` as a reference only while the server is built.
- `sdk/hooks-types/` (`.d.ts` for hooks) ships from the server repo; the client SDK does not.

---

## 4. Data model & schema

### 4.1 Collections

- **Each collection = one real Postgres table** (no EAV / JSONB for primary data) so indexes,
  FKs, RLS and the planner all work.
- System metadata uses `_`-prefixed tables (no schema prefix):

| Table | Purpose |
|---|---|
| `_collections` | id, name, type (base/auth/view), options jsonb, rules, created, updated |
| `_fields` | collection_id, name, type, required, unique, options jsonb, position |
| `_settings` | key/value app settings (SMTP, S3, CORS, rate limits, ...) |
| `_crons` | persisted cron jobs |
| `_migrations` | applied migration ledger |
| `_sessions` | refresh tokens (hashed), rotation/reuse detection |
| `_jobs` | job queue |
| `_file_outbox` | post-commit S3 delete queue |
| `_image_presets` | named resize presets |
| `_rt_log` | short-term realtime replay log (resume via `Last-Event-ID`) |
| `_logs` | request logs |
| `_audit` | admin action audit log |

Identifiers are strict: collection and field names must match `^[a-z][a-z0-9_]*$`, be at most 63
bytes, and must not start with `_` (reserved for system tables). They are validated *before* being
quoted into DDL, and are never silently normalised — `create("Posts")` is an error, not a
collection quietly named `posts`.

- User data lives in the `public` schema (or `app`; never hard-coded — section 14).

### 4.2 Field types (PocketBase parity)

text, editor (Markdown rich text), number, bool, email, url, date/datetime, select
(single/multi), file, relation (single/multi), json, geo-point, autodate, password (auth only).

### 4.3 IDs and default columns

- `id`: 15-char base36/ULID-short, generated server-side (small index, URL-friendly).
- `created`, `updated`: `timestamptz`.
- `collectionId`, `collectionName` are exposed on records (PocketBase parity).

### 4.4 Collection types

- **Base**: a normal table.
- **Auth**: adds email, password_hash, verified, token_key, OAuth2/MFA columns.
- **View**: a Postgres `VIEW` from user-written SQL (read-only), created
  `WITH (security_invoker = true)` so RLS on the source tables still applies (section 6.3).

### 4.5 Migrations

- **System** schema changes come from `migrations/` (versioned SQL, in-repo), applied by the
  migration runner under `pg_advisory_lock`.
- **User** schema changes (Studio collection edits) produce SQL migrations recorded in
  `_migrations` and exportable to `migrations/` for git.
- Safe ALTERs only: add nullable column → backfill → set not null; avoid long locks.

### 4.6 Schema cache

- Each node holds an in-RAM schema cache (collections, fields, rules) as
  `Arc<ArcSwap<Schema>>`.
- On a schema change: the node applies it → `NOTIFY schema_changed` → every node reloads.
- Lock-free on the read path.

---

## 5. Generated API (PocketBase-canonical)

### 5.1 Endpoints

```
GET    /api/collections/{c}/records        list (filter, sort, page, fields, expand)
GET    /api/collections/{c}/records/{id}   view
POST   /api/collections/{c}/records        create (JSON or multipart)
PATCH  /api/collections/{c}/records/{id}   update
DELETE /api/collections/{c}/records/{id}   delete
POST   /api/batch                          multiple operations in one transaction
GET    /api/collections                    collections meta API
POST   /api/collections/{c}/auth-with-password
POST   /api/collections/{c}/auth-refresh
GET    /api/realtime                       SSE
GET    /api/files/{collectionId}/{recordId}/{filename}
GET    /api/health
GET    /api/logs          (superuser)
GET    /api/settings      (superuser)
GET    /api/crons         (superuser)
GET    /api/backups       (superuser)
```

List responses are PocketBase-shaped: `{ page, perPage, totalItems, totalPages, items }`.
Pagination uses `page`/`perPage`/`skipTotal`, with optional cursor/keyset (`after=<cursor>`)
for big tables. `GET .../records` accepts `filter`, `sort`, `fields`, `expand`.

### 5.2 Query language

- `filter`: PocketBase expressions:
  `title ~ "abc" && (status = "a" || views > 10) && author.name = "x"`
  - Hand-written lexer + precedence-climbing parser → AST → **fully parameterized SQL**. Values
    are never interpolated; field names are resolved against a schema-derived whitelist, so an
    unknown field is an error rather than a raw column reference.
  - Operators: `= != > >= < <= ~ !~ ?= ?!=`, plus `@now` and `@request.auth.*`.
  - `~` / `!~` are **case-insensitive** contains (`ILIKE`), matching PocketBase; `%` and `_` in
    the value are escaped so they stay literal.
  - `?=` / `?!=` target `text[]` columns and are *required* for them — comparing an array with `=`
    is refused with a pointer to `?=`.
  - Parameters are sent as text and cast (`$n::text::bigint`): one homogeneous parameter type,
    while the comparison still gets the type it needs.
  - Dotted paths (`author.name`) become `exists (...)` subqueries, including multi-relations via
    `any(...)`. Sorting through a relation is not supported yet.
  - Filters are bounded: token count, nesting depth and path depth.
- `sort`: `-created,title`. Only real columns are accepted; a dotted sort path is refused loudly
  until joins land with `expand`.
- `fields`: column selection; a dotted entry (`author.name`) narrows the document of an expanded
  record. An unknown field, or a path that does not go through a relation, is an error.
- `expand`: `expand=author,comments.user`, or `*` for one level. Relations are expanded with
  **correlated subqueries** rather than `LEFT JOIN LATERAL`: one row per record, no row
  multiplication for multi-relations, and `jsonb_agg(... order by ...)` keeps arrays ordered.
  Depth is capped (≤ 6) and the rows inside one expanded multi-relation are capped.
- Paging: `page`/`perPage`/`skipTotal` (PocketBase), plus an opt-in keyset cursor
  (`after=<opaque token>`) over `(created, id)` for large tables. The two cannot be mixed, and a
  cursor is only valid with the default ordering.
- Full-text: `tsvector` + GIN.

### 5.3 Hot-path latency

1. **Prepared-statement cache** keyed by (collection, query shape). `tokio-postgres` names
   every prepared statement and `deadpool-postgres` keeps it across checkouts, so lazypock
   **requires a direct (non-transaction-pooled) Postgres connection** — see 18.1.
2. **JSON built by Postgres**; Rust only pipes bytes.
3. Small pool (5-10 per instance); one short transaction per request.
4. Response compression (`zstd`/`br`/`gzip`) for large payloads, skipped for small ones.
5. HTTP/2, keep-alive, `TCP_NODELAY`.
6. Avoid clone/alloc: `Bytes`, `Cow`, small arenas for the parser.
7. ETag / `If-None-Match` for single-record views (based on `updated`).

---

## 6. Permissions: Postgres RLS

### 6.1 Per request

```sql
BEGIN;
SET LOCAL ROLE app_authenticated;                      -- or app_anon, app_service
SELECT set_config('request.auth.id',     $1, true);
SELECT set_config('request.auth.claims', $2, true);    -- JSON claims
SELECT set_config('request.connection_id', $3, true);  -- for realtime (8.7)
SELECT set_config('request.action',      $4, true);  -- list | view | update | delete | write
-- the user's query runs here (RLS applies automatically)
COMMIT;
```

- `set_config(..., true)` is transaction-local, so it is **safe with connection pooling**.
  `SET LOCAL ROLE` also resets at commit, so it is safe with PgBouncer transaction pooling.
- The server connects with a dedicated **non-superuser** role that is a member of
  `app_anon`, `app_authenticated`, `app_service`. `app_service` bypasses RLS and is internal.
- Tables are created with `ENABLE ROW LEVEL SECURITY` but **not** `FORCE ROW LEVEL SECURITY`:
  forcing it would subject the schema owner to the policies too and break the server's own
  internal reads (the login lookup is the obvious one). The owner *is* the service identity —
  internal work and superusers run as it and bypass policies, while every user-facing request
  runs as a restricted role. Migration `0003_rls.sql` carries the same note.
- SQL helpers: `auth.uid()`, `auth.claim('role')` reading `current_setting`.

**Implementation note.** The names in this section are the design's; the code uses
`lazypock_anon` / `lazypock_authenticated` (`migrations/0003_rls.sql`) and models the service
bypass as the connecting **owner** role rather than a separate `app_service`.

**`request.action` and the list/view divergence (resolved).** `listRule` and `viewRule` both
compile to a `SELECT` policy, and Postgres OR-combines permissive policies — so a public
`viewRule` used to widen a filtered `listRule` (and vice versa). Each `SELECT` policy is now
guarded by `request.action`, set per request from the operation the endpoint performs. Two
consequences worth stating:

- `updateRule` and `deleteRule` also get a `SELECT` mirror keyed to their action
  (`lazypock_update_select` / `lazypock_delete_select`), because Postgres applies the `SELECT`
  policies to the rows an `UPDATE`/`DELETE` reads to find its target. The mirror carries the
  rule's own predicate, so a row is writable exactly when its rule says so — and never because
  of a wider `listRule`/`viewRule`.
- `write` is the default action and matches no `SELECT` policy: a read that forgets to declare
  itself is **denied** rather than silently matching a different rule. Every restricted-role
  `SELECT` path must set `list`, `view`, `update` or `delete` explicitly (records endpoints,
  batch sub-requests, and realtime subscriptions — a collection subscription is a `list`, a
  record subscription a `view`).

**Known divergence: expanded relations.** `expand` resolves relations with correlated
subqueries inside the same statement, so a `list` request's expanded rows are filtered by the
related collection's **`listRule`**, not its `viewRule` (the reference resolves each expanded
record through its `viewRule`). The old behaviour ORed list+view, so this is *narrower*, not a
leak — but it is a real semantic difference. Fixing it needs the expand subqueries to run with
a view action, which a single statement-wide claim cannot express; that is a separate piece of
work.

### 6.2 Superuser and `manageRule` mapping (resolved)

The Elixir rule semantics to preserve: `nil` = superuser only, `""` = public, filter =
conditional, plus a `manageRule` that grants all actions.

- **Superuser bypass**: superuser requests run as `app_service` **only after** the auth layer
  has verified the token's `is_superuser` claim. `app_service` is never reachable from an
  end-user token.
- **`manageRule`**: compiled into an additional permissive policy with the same
  `USING` + `WITH CHECK` expression, so it grants `SELECT/INSERT/UPDATE/DELETE` at once.
- **`nil`**: no policy is created ⇒ `app_anon`/`app_authenticated` see nothing (default deny).
- **`""`**: policy with `USING (true)` / `WITH CHECK (true)`.

### 6.3 Rule editor in Studio

- Pick collection → action (list/view/create/update/delete) → build the condition via UI or a
  simple expression (`author = @auth.id`).
- The server compiles it to `CREATE POLICY ... USING (...) WITH CHECK (...)`, stores the source
  in `_collections.rules`, and applies `ENABLE ROW LEVEL SECURITY` (not `FORCE`; see 6.1).
- "Advanced" mode edits the policy SQL directly.
- Templates: public read, owner only, authenticated only, role-based.
- **View collections** are created `WITH (security_invoker = true)`; otherwise the view owner's
  rights would bypass RLS on the source tables.
- **"Test as user"** button: choose a user/claims, run a query, see what RLS returns.

### 6.4 RLS performance

- Policies must be sargable with indexes on the columns they use (Studio warns when an index
  is missing).
- Use `(SELECT auth.uid())` so the planner caches the value (initPlan).
- Field-level permissions: column privileges or views; RLS only gates rows.

---

## 7. Auth

### 7.1 Features (PocketBase parity)

- Email/password (argon2id), verify email, reset password, change email.
- OAuth2 (Google, GitHub, Apple, Discord, ...) through a provider trait; code flow + PKCE.
- Email OTP, MFA (TOTP), passkeys (WebAuthn) later.
- Impersonate (superuser signs in as a user).
- Multiple custom auth collections.
- API keys (hashed, shown once) for codegen/unauthenticated collection listing.

### 7.2 Tokens & sessions

- Short-lived JWT access token (e.g. 15 min, EdDSA) + refresh token stored as a **hash** in
  `_sessions` (rotation, reuse detection).
- Signing keys in settings, rotation supported (JWKS with multiple `kid`s).
- Revocation: refresh tokens live in `_sessions` (rotation, reuse detection), and a password change
  revokes the account's sessions — that is what ends the tokens issued before it. `token_key` is a
  per-account column (PocketBase's `tokenKey`) that is rotated on a password change and carried in the
  access token as its `tkey` claim, but nothing verifies it: an access token is checked by signature
  until it expires, and refresh is keyed on `_sessions`, not on the key. It is therefore **not a
  field** (migration 0010) — a client, and the Studio, never sees it.

### 7.3 External auth (bring your own IdP)

- Configure `issuer`, `jwks_uri`, `audience`, claim → `auth.id`/role mapping.
- JWKS cached (TTL + refresh on unknown `kid`).
- External users mapped / auto-created (JIT provisioning) in an auth collection.
- Option to use external auth only (disable built-in auth).

### 7.4 Protection

- Rate limits (login, OTP, reset) per IP + account; in-memory per node with optional shared
  DB counter for sensitive endpoints.
- Constant-time comparison, soft lockout, CORS, CSRF for cookie mode, security headers.

---

## 8. Realtime

### 8.1 Client protocol

- **SSE** at `/api/realtime` (PocketBase-canonical; simple, proxy-friendly). WebSocket optional.
- Subscribe by `collection/*`, `collection/{id}`, with an optional `filter`.
- Events: `create | update | delete`; payload is the record (RLS applied).
- Heartbeat every 25-30 s; `Last-Event-ID` for **resume** after reconnect.

### 8.2 Event source: Postgres → node

**Triggers + LISTEN/NOTIFY**:

```sql
PERFORM pg_notify('rt', json_build_object(
  'c', TG_TABLE_NAME, 'op', TG_OP, 'id', NEW.id, 'v', txid_current(),
  'o', nullif(current_setting('request.connection_id', true), '')
)::text);
```

- Each node keeps **one dedicated LISTEN connection** (outside the pool).
- **NOTIFY payload limit is 8000 bytes** → send only `collection + op + id (+ updated + origin)`;
  never the whole record. The node fetches the record when needed.
- NOTIFY is delivered at COMMIT, in commit order: good for consistency.

Upgrade path: **logical replication** (`pgoutput`) — one replication slot/consumer, no missed
events on restart, full payload, higher throughput. More complex (slots, WAL, single consumer ⇒
leader election). Switch only if NOTIFY is not enough.

### 8.3 Fan-out inside a node

```
Postgres NOTIFY ──► Listener task ──► Dispatcher
                                         │ subscription table: DashMap<collection, Vec<SubId>>
                                         ▼
                      Per event: filter subs by collection/record/filter
                                         ▼
                      Permission check (RLS) ──► push into each client's bounded mpsc channel
```

- Each client has a **bounded channel**; when full ⇒ drop the oldest event or disconnect.
- One dispatcher task, several writer tasks; never hold locks across `.await`.
- Limits: subscriptions per client, clients per node (from RAM).

### 8.4 Realtime security with RLS

Clients must only receive records RLS lets them see.

- On an event, **group subscribers by identical claims** (same `auth.id`/role). For each group
  run **one** `SELECT ... WHERE id = ANY($ids)` under that role/claims; a returned row ⇒ visible.
- **Batch** events in a short window (5-20 ms) into one query per group.
- `delete`: the row no longer exists ⇒ send it to whoever has seen the record (bounded LRU of
  ids the subscription recently received), or capture the policy columns in the trigger.
- Anonymous/public collections skip the check.
- Short-lived cache (a few seconds) of check results by (claims, id, version).
- Per-collection setting: `realtime = off | ids_only | full`.

### 8.5 Horizontal scaling

- Every node LISTENs independently ⇒ **every node receives every event** and pushes only to its
  own clients. No sticky sessions for the write path.
- SSE/WS clients are pinned to one node; on node death the client reconnects elsewhere and
  resumes with `Last-Event-ID`.
- **Resume**: `_rt_log` (monotonic id, collection, op, record id, origin, created), kept
  short-term (e.g. 5 min, cleaned by cron).
- NOTIFY cost grows with nodes × events; a few dozen nodes are fine.
- **Backpressure**: per-collection event rate limits; coalesce consecutive updates to one record.
- Metrics: connections, subscriptions, commit→client latency (p50/p99), dropped events, queues.

### 8.6 Mandatory load tests

- Initial target: 10k SSE connections per node with < 10-20 KB RAM per connection (excluding
  buffers). Test with k6 / a custom tool before locking the design.

### 8.7 Connection-id filtering (no echo to the originating device)

Requirement: user A does CRUD on device 1 ⇒ device 1 does not receive the realtime event for
that operation, but device 2 of the same user A does. **This is an intentional divergence from
PocketBase**, which delivers the event to the initiator.

- The client generates a random `connectionId` (128-bit) per app instance (device/tab) and keeps
  it across reconnects.
- Sent when opening SSE: `GET /api/realtime?connectionId=<id>`.
- Every write sends `X-Connection-Id: <id>`; the SDK attaches it automatically.
- The write transaction runs `set_config('request.connection_id', $cid, true)` alongside claims.
- The trigger puts it in the NOTIFY payload as `origin`, and `_rt_log` stores it for replay.
- Writes from hooks/jobs/direct SQL ⇒ `origin = null` ⇒ **all** subscribers receive it.
- Dispatcher skips any subscriber whose `connectionId == event.origin` — a cheap compare done
  **before** the RLS check, so it also saves queries.
- Multi-node: only the node holding that connection has a matching subscriber.
- `connectionId` is not an auth secret; it is a filter hint only and changes no permissions.
- Optional per-subscription `echo=true` restores PocketBase behavior.
- Required tests: same user with two connections, missing/invalid header, hooks/jobs writes,
  multi-node, resume after reconnect.

### 8.8 Redis: analysis and decision

**Recommendation: do not take a Redis dependency now. Keep Postgres as the bus, behind an
`EventBus` trait so a Redis backend can be added without touching call sites.**

Why Postgres-first:

- The default deploy ships Postgres in `docker-compose`; on any standard/self-hosted Postgres,
  LISTEN/NOTIFY is fully supported. The Neon/pooler caveat only applies to *hosted pooled*
  endpoints, where the direct endpoint still works for the single LISTEN connection.
- Zero extra sidecars: one less process to install, secure, monitor and pay for on a 2 GB VPS.
- The job queue (`FOR UPDATE SKIP LOCKED`), cron locks (`pg_try_advisory_lock`), rate limits and
  `_rt_log` are all already solved in Postgres, and were proven in the Elixir implementation.

What Redis would buy, and when to switch:

| Redis benefit | Trigger to adopt |
|---|---|
| Pub/sub fan-out without the 8 KB NOTIFY limit | payloads/realtime volume outgrow ids-only, or dozens of nodes |
| Durable replay + queue (`Streams`, consumer groups) | `_rt_log` + `SKIP LOCKED` latency becomes the bottleneck |
| Works behind PgBouncer transaction pooling / Neon scale-to-zero | deploying on Neon with pooled endpoints is a hard requirement |
| `SET NX PX` locks, `INCR`+`EXPIRE` rate limits, caching | shared exact rate limits across nodes are required |

Implementation plan: define `trait EventBus { publish(event); subscribe(...) }` in `realtime`,
with `PgBus` as the default implementation. A `RedisBus` can be added later behind a
`--features redis` flag. Postgres remains the source of truth for **data** in all cases; Redis
would only ever be transport/queue/cache.

**Packaging risk (flagged):** adding Redis is a sidecar dependency for the single-binary /
Docker deployment story. That is why it is deferred rather than adopted now.

---

## 9. Overall horizontal scaling

| Concern | Solution |
|---|---|
| Session state | None in the node; JWT + `_sessions` in the DB |
| Migrations | `pg_advisory_lock` at startup; only one node runs them |
| Cron | Each job takes `pg_try_advisory_lock(hash(job))` ⇒ only one node runs each tick |
| Job queue | `_jobs` + `SKIP LOCKED`, retry, backoff, dead-letter; wake-up via `NOTIFY jobs` |
| Rate limiting | In-memory per node by default (approximate); optional shared DB counter |
| Schema cache | `ArcSwap` + NOTIFY invalidation |
| DB connections | Small pool per node; total = nodes × pool ≤ `max_connections` |
| Uploads | Streamed straight to S3/R2; the node never holds files in RAM/disk for long |
| Hooks hot-reload | Each node watches its folder; in cloud, hooks live in DB/S3 and sync via NOTIFY |
| Health | `/api/health` (liveness), `/readyz` (DB check), graceful shutdown (drain SSE, stop jobs) |
| Deploys | Rolling updates; backward-compatible migrations (expand → migrate → contract) |
| Heavy reads | Later: read replica for `GET`s that don't need read-your-writes (config flag) |

---

## 10. Files & image service

### 10.1 Upload

- **Images only.** Validate **magic bytes** (never trust `Content-Type`/extension), default max
  size (e.g. 5 MB); **allowed types and size configurable per field in Studio**.
- Reject SVG or sanitize separately (XSS risk); cap pixel dimensions (decompression-bomb defense)
  by reading headers before decoding.
- Stream multipart → S3/R2 (multipart upload for large files). Key layout:
  `{collection}/{record_id}/{random}_{sanitized-filename}`.
- Optional presigned upload (client uploads straight to R2, server confirms afterwards).
- Storage settings in Studio: endpoint, bucket, region, key, secret, public base URL/CDN;
  "Test connection" button. Secrets encrypted at rest.

### 10.2 File ↔ record lifecycle

- Deleting a record / clearing a file field ⇒ write to `_file_outbox` **in the same transaction**
  (trigger or code). A worker deletes the S3 objects after commit and retries on failure ⇒ no
  orphans, no wrong deletes on rollback.
- With a dedicated `files` table (media library): deleting the row ⇒ outbox removes the original
  and its variants.
- Periodic job: reconcile S3 objects with no references (orphan sweeper), with dry-run mode.

### 10.3 Image service (separate)

Why separate: resizing needs peak RAM/CPU; isolating it lets you cap it with cgroups, scale it
independently, and keeps the API server from OOMing.

Choice: **imgproxy** (Go + libvips) in `docker-compose`, R2 as origin, CDN in front. Tune
`IMGPROXY_MAX_SRC_RESOLUTION`, `IMGPROXY_CONCURRENCY`, container memory limits. A custom Rust
service is only worth it if imgproxy falls short.

```
Client ─► CDN ─(miss)─► Image service ─► R2 (origin) ─► resize ─► respond + Cache-Control: immutable
```

- HMAC-signed URLs (`/img/{sig}/{preset}/{path}`) to prevent arbitrary-resize abuse.
- **Presets in the DB** (`_image_presets`: name, width, height, fit, format, quality), editable
  in Studio, exposed to the image service.
- Output format: WebP/AVIF per `Accept`, JPEG fallback.
- Concurrency limit (semaphore), timeouts, max source size.
- **Pre-generated "content" variant**: richtext images get one variant (~1280 px WebP) generated
  at upload by a background job; the CDN URL goes into the Markdown instead of the original.
- On delete: purge the CDN cache (Cloudflare API) or use versioned URLs.
- Studio thumbnails use a fixed `thumb` preset.

### 10.4 Richtext (TipTap) in Studio

- Stores Markdown (`getMarkdown`); inserted images use the CDN URL of the `content` variant.
- Picker: choose an already uploaded file or upload a new one ⇒ returns the variant URL.
- Deleting a file that is in use: warn (find references with LIKE on the URL, optional).

---

## 11. TypeScript hooks (no recompile)

### 11.1 Mechanism

- Folder `hooks/*.pb.ts` (or stored in the DB in cloud mode). The server uses `oxc` to **strip
  types** → JS → runs in **QuickJS**.
- File watcher: editing a file reloads the runtime immediately (hot-reload). A compile error never
  crashes the server: keep the previous version and log the error.
- Pool of runtimes (one context per worker thread) for parallelism; limits: **memory
  (16-32 MB/context), execution time (2 s), stack**.
- With no hook files ⇒ **the runtime is never initialized** (saves RAM).

### 11.2 Example API

```ts
onRecordBeforeCreate("posts", async (e) => {
  e.record.slug = slugify(e.record.title);
});
onRecordAfterUpdate("orders", async (e) => {
  await $http.post("https://example.com/hook", { id: e.record.id });
});
routerAdd("GET", "/api/hello/:name", (c) => c.json(200, { hi: c.param("name") }));
cronAdd("daily", "0 3 * * *", () => { /* ... */ });
```

- Events: before/after create/update/delete, auth events, request hooks, mailer hooks, realtime
  hooks.
- Bindings: `$db` (parameterized queries inside the request's transaction), `$http`, `$os`
  (restricted), `$mails`, `$app.settings()`.
- `.d.ts` shipped in `sdk/hooks-types`.

### 11.3 Security & performance

- Sandbox: no `fs`/`net` by default; exposed through controlled APIs; domain allowlist for `$http`.
- "Before" hooks run **synchronously inside the transaction** ⇒ keep them short; "after" hooks can
  run through the job queue so they don't block the response.
- Cloud/multi-tenant: per-tenant hook limits.

---

## 12. Studio (SvelteKit, bun, embedded in the binary)

### 12.1 Build & embed

- `studio/` uses **bun** (`bun install`, `bun run build`) and `adapter-static` (SPA mode,
  fallback `index.html`).
- `crates/studio-embed/build.rs`: runs the bun build (or uses a prebuilt bundle in CI),
  compresses with brotli, embeds with `rust-embed`.
- Served at `/_/` with long cache for hashed assets, `index.html` no-cache; serves `.br` when
  the client supports it.
- Dev: `bun run dev` proxies `/api` to the Rust server for HMR.

### 12.2 Screens

- Collections: create/edit fields, indexes, rules (RLS editor), API preview (curl/SDK snippets).
- Records: grid, filter, sort, inline edit, CSV/JSON import/export, richtext editor.
- Auth: users, OAuth2 providers, MFA, email templates, API keys.
- Files: media library, image presets (CRUD, preview).
- Realtime: connections/subscriptions monitor, live event tail.
- Logs & metrics: request logs, hook errors, slow queries.
- Settings: SMTP, S3/R2, backup, CORS, rate limit, external auth, hooks editor (Monaco).
- Migrations: history, export, apply.
- "Test as user" for RLS.

The existing `studio/` is the reference implementation of all of the above; the rebuild points it
at the Rust API and swaps the realtime transport to SSE.

### 12.3 Studio authentication

- Separate superuser identity, distinct from normal users; MFA supported; optional IP restriction.
- CLI: `lazypock superuser create`.

---

## 13. Parity backlog

- Backup/restore (logical `pg_dump` or DB snapshot + S3 sync), scheduled backups, rollback
  checkpoints on import.
- Email (SMTP, templates, send queue).
- Request logs (async batched writes, retention) + stats.
- Configurable per-route rate limiting.
- CORS, trusted proxy headers, optional automatic TLS (or leave to Caddy).
- Batch API, transactions in hooks.
- Webhooks / outbound events.
- Settings import/export + versioning.
- **Studio ↔ server endpoint gaps** (surfaced by the e2e tour, section 15): the Studio
  calls `POST /api/sql/query` (SQL Console), `POST /api/collections/meta/dry-run-view` and
  `POST /api/collections/meta/preview-view-builder` (view-builder preview), and
  `POST /api/settings/storage/test` (Files → "Test connection") — none of which the Rust
  server implements yet, so the page renders but the action 404s. The SQL console needs a
  superuser-only, read-only (SELECT/EXPLAIN/WITH) endpoint; the storage test needs a real
  backend round-trip. `studio/e2e/studio.spec.ts` carries the SQL console as a `test.fixme`
  until the endpoint exists.
- **Invalid-credentials status.** `main` answers **400** on a failed `auth-with-password`; this server
  answers **401** (pinned by `a_malformed_login_body_is_rejected_clearly`). 401 is more correct HTTP,
  but it is a client-visible difference. Confirm before changing.
- **Auth system collections are presented, not stored.** `_auth_origins`, `_external_auths`, `_mfas` and
  `_otps` are synthesised in the admin list for Studio parity; their tables do not exist yet because
  OAuth2, MFA and OTP are not ported (section 7.1). When those land, create the tables and register
  them as real system collections, which also makes them browsable (today a click has no records to
  show). Serving them will need the same default-deny as every other collection: the `_otps` and
  `_external_auths` rows are credential-adjacent, so RLS has to be enabled with the migration rather
  than left off. The names follow the existing snake_case convention (`_auth_origins`), not PocketBase's
  camelCase (`_authOrigins`); confirm before creating the tables.
- PocketBase data importer.
- View collection builder (no-code) + ERD.
- Audit log for admin actions.
- Direct-to-bucket uploads, per-field file policy, file reference tracking.
- CLI: `serve`, `migrate up/down`, `superuser`, `export schema`, `healthcheck`.
- JS/TS SDK (Dart/Swift/Kotlin later if needed).

---

## 14. PocketHost-style cloud hosting (later)

Not built now, but the design must not block it:

- **Tenant model: one database per tenant + one process per tenant.** Strong isolation; simple
  backup/delete (dump or drop a whole DB); independent migrations; fits Neon (one project/branch
  per tenant, scale-to-zero); NOTIFY/advisory locks need no tenant prefix. The binary is light, so
  many processes per VPS stay feasible.
- The control plane keeps the tenant catalog (DB URL, subdomain, quota) in its own control DB.
- Cost: each DB has its own catalog and connections ⇒ keep pools tiny (1-3 + 1 LISTEN per tenant).
  Total connections = running tenants × (pool + 1).
- Fallback if the free tier must be cheaper: several tenants share one DB with per-tenant schemas,
  using `search_path` + prefixed NOTIFY channels. Hence **never hard-code the `public` schema**.
- Control plane: create tenants, provision DB (Neon API), assign subdomain, start/stop
  processes, billing, quotas.
- **Scale-to-zero**: stop idle instances, wake on request; startup must stay under ~200 ms.
- Per-tenant cgroup limits, storage/bandwidth quotas.
- Image service shared across tenants (separate bucket/prefix + per-tenant signatures).
- Config via env/flags so the control plane can generate it easily.

---

## 15. Testing, measurement, CI

- **Unit tests**: filter parser, query builder, rule → policy compiler, ID generation, config.
- **Integration tests**: real Postgres (Docker/testcontainers), RLS across roles, realtime
  end-to-end, hooks sandbox, S3 via a local mock (MinIO).
- **Property/fuzz tests**: `proptest`/`cargo-fuzz` for the filter parser and upload validator.
- **Benchmarks**: `criterion` for parser/builder; `k6`/`oha` for the API; N-connection realtime.
- **Track** binary size, idle RAM, RAM under load N, p50/p99 latency — **wired into CI as
  regression thresholds** against a recorded baseline.
- **CI**: `cargo fmt --check`, `cargo clippy -D warnings` (fix at the root, no blanket allows),
  `cargo deny` (license/CVE), `cargo test`, multi-arch builds (x86_64/aarch64 musl), releases.
- Known CI trap to avoid: pipelines must fail on test failure (no `| tee` without `pipefail`).

---

## 16. Security overall

- All SQL parameterized; identifiers whitelisted from the schema cache.
- Least-privilege DB roles; RLS on by default (ENABLE + FORCE); secrets encrypted at rest.
- Limits on body size, field count, expand depth, filter complexity.
- Security headers, audit log for admin actions, no native/loose `confirm`-style flows in Studio
  (in-app dialogs only).

---

## 17. Tracks

Independent tracks, so progress and estimates do not entangle:

| Track | Scope | Depends on |
|---|---|---|
| **A. Foundations** | workspace, config, errors, tracing, CLI, health, tests harness | — |
| **B. Data plane** | pool, migrations, system tables, schema engine, DDL, schema cache | A |
| **C. Query plane** | filter parser/compiler, sort, pagination, fields, expand | B |
| **D. Records API** | CRUD, validation, batch, ETag | C |
| **E. Access control** | auth, JWT/sessions, roles, RLS policies, rule editor codegen | B, D |
| **F. Realtime** | EventBus, triggers, LISTEN, SSE, connection-id, replay | E |
| **G. Files & images** | upload, adapters, outbox, presets, imgproxy | D |
| **H. Hooks** | QuickJS, TS strip, bindings, hot reload | D |
| **I. Jobs & ops** | jobs, cron, mailer, logs, backup, rate limit | B |
| **J. Studio** | rebuild against Rust API, embed | D, E, F |
| **K. SDK** | rebuild `lazypock-ts` | D, E, F |
| **L. Deploy** | Docker, compose, CI, docs, release | all |
| **M. Cloud** | control plane, tenants, scale-to-zero | all |

#### Hook bindings: why `$db` is not a small change

The hook runtime is not `Send` (QuickJS holds an `Rc`), so it runs on its own thread
and the request reaches it over a channel. That means a hook **cannot** use the
request's transaction: the transaction lives on the request's connection, which
belongs to a different thread.

So `$db` has to run on the hook thread with its own connection, and its writes
commit independently of the record write. A before-hook's `$db` write therefore
would *not* roll back with the record, which contradicts the "`$db` inside the
request's transaction" in section 11.2. That is a real semantic decision rather
than plumbing, so it is left undone rather than half-done, and `$http` additionally
needs the domain allowlist from section 11.3 before it is safe to expose.

### Configuration convention

**Every environment variable lazypock reads is prefixed `LAZYPOCK_`.** The one
deliberate exception is `RUST_LOG`, which belongs to the `tracing` ecosystem and is
read through `EnvFilter::try_from_default_env`; `LAZYPOCK_LOG_LEVEL` is the
supported knob and takes the same values. A bare `DATABASE_URL` used to be
accepted as a fallback and no longer is: that name is set by plenty of other
tooling in the same environment, and honouring it could quietly connect lazypock to
somebody else's database.

## Implementation status

| Track | State |
|---|---|
| A. Foundations | done — workspace, config (fail-loud, secrets redacted), errors, ids, telemetry, CLI, `/api/health` |
| B. Data plane | done — pool, advisory-locked + checksummed migration runner, `_`-prefixed system tables, field types/validation, DDL engine, collections repository, `ArcSwap` schema cache. TLS is implemented (rustls + system trust store): `sslmode=require`/`verify-ca`/`verify-full` and the `ssl=require` spelling are honoured with **certificate verification**, verified against Neon. |
| C. Query plane | done — filter lexer/parser, schema-whitelisted parameterized compiler, `sort`, `fields`, `expand`, offset + keyset paging, and a complete record `SELECT` with Postgres-built JSON. |
| D. Records API | done — `GET/POST/PATCH/DELETE /api/collections/{c}/records(/{id})` with the PocketBase list envelope, per-field validation details, ETag/`If-None-Match`, filter/sort/fields/expand/paging wired to the query plane, and `POST /api/batch` (all-or-nothing, one transaction). |
| E. Access control | done — argon2id, EdDSA tokens with a published JWKS, sessions with rotation and replay detection, superusers, the CLI, the auth HTTP endpoints behind a bearer middleware, accounts in auth collections (a password is optional, matching `main`, so OAuth/OTP/invite-only accounts exist), and **Postgres RLS** for collection rules (roles, per-request `set_config` context, rule → `CREATE POLICY`, default deny). A password change revokes the account's `_sessions` and rotates its `token_key`; the revocation is what ends the refresh tokens issued before it. |
| F. Realtime | done — the change-event trigger plus `_rt_log`, the `EventBus` seam with a Postgres `LISTEN` implementation, RLS-filtered fan-out with connection-id echo suppression, and SSE at `/api/realtime` with `lastEventId` resume. The Redis backend, event batching and logical replication are deliberately deferred (section 8.8). |
| G. Files & images | **done** — upload validation, the key layout, local and S3 backends, the delete outbox, the HTTP upload/serve/delete routes, image presets with cached thumbnails, a superuser-gated CRUD API for them, and an orphan sweeper (**resized in-process, so no `imgproxy` sidecar and no extra artifact to ship — a deliberate change from section 10**). Two known limits: AVIF output is refused loudly because the schema allows it and the encoder cannot write it, and the S3 path has **not** been exercised against a live bucket (signing and listing are unit-tested; a live round-trip needs credentials and a bucket). |
| H. Hooks | **partly** — the thread-owned worker, the six events, TypeScript stripping, the loader with hot reload and the `RecordHooks` seam are in, and **create, update and delete all run their before-hooks and after-hooks** (before runs before validation so a hook can supply a field; after runs post-commit; a before-delete hook can refuse by throwing). Remaining: the `$db`/`$http`/`$app` bindings — see the note below on why `$db` is not a small change. |
| I. Jobs & ops | **partly** — request logs are in end to end: `_logs` aligned by migration 0007, a recorder middleware that redacts credential keys and captures error messages, and `GET /api/logs`, `/logs/stats`, `/logs/collections`, `/logs/retention`, `PATCH /logs/retention`, and `DELETE /logs?all=true` and `DELETE /logs?days=n`. The cron **admin API** is in too (`GET/POST/PATCH/DELETE /api/crons`, `POST /crons/validate`, and `POST /crons/{id}` run-now for `sql`/`http`; a `hook` job refuses loudly). Remaining: the cron **scheduler** that fires on `next_run_at`, the job queue, mailer and rate limits. |
| J–K. Studio, SDK | **done except the JSON import body.** The SDK is ported and its live integration test passes; the collection admin API exists and serves the shape the Studio reads (`schema` alongside `fields`, per-field `hidden`/`system`/`indexed`, RFC 3339 `created`/`updated`, top-level `indexes`/`view*`); the PocketBase system collections (`_superusers`, `_auth_origins`, `_external_auths`, `_mfas`, `_otps`) are listed as locked entries — synthesised, so they stay out of the schema cache; an auth record's password is accepted under PocketBase's `password` **or** the password field's own name, which is what the Studio's generated form sends, and is hashed either way; `GET /api/export` serves the single-document JSON; logs, crons, API keys (`GET/POST/DELETE /api/settings/api-keys`, hashed, scoped, shown once) and `GET /api/import/status` / `POST /api/import/rollback` exist; and backup/restore is complete end to end — `GET /api/export/archive`, `POST /api/import`, `GET /api/import/preflight` — with the documented archive format and both safety invariants enforced server-side (an import never deletes; never writes a `_`-prefixed system collection). The Studio's import page defaults `deleteMissing` to **false** (the destructive case is opt-in, matching the published docs), so the only remaining item is that `POST /api/import` does not yet accept the JSON body a downloaded JSON backup is restored with. |
| L. Deploy | **partly** — boot-time auto-migration works on an empty database and on one that already holds tables, and an existing ordinary database is **reflected**: every table becomes a collection and every column a field, verified against a real database. Remaining: the GitHub Action that builds and publishes the image, and the compose/release docs. |
| M. Cloud | not started |

Deleting a collection can keep its data (`drop_data = false`); re-declaring it with
`adoptExistingTable: true` attaches to the kept table, and the adoption is refused when the table's
columns do not match the declaration — so metadata can never misdescribe the real schema.

A configured constraint that cannot yet be enforced is refused rather than stored: a text field
with a `pattern` option is rejected at collection creation, so no collection can carry a rule that
is silently not applied.

The schema cache is loaded at boot and refreshed live: every schema mutation emits
`NOTIFY lazypock_schema_changed` at commit, and each node watches it on a dedicated connection.
The watcher also reloads once immediately after `LISTEN`, which closes the window between the
boot-time load and the watch starting. A schema change made by hand in SQL does not emit that
notification, so a direct DDL change still needs an explicit reload or a restart.

Two security notes for the auth layer:

* The access-token signing key is generated once and stored in `_settings` **in plaintext**.
  Encrypting it at rest, or supplying it through `LAZYPOCK_SIGNING_KEY`, is a tracked follow-up.
* Access tokens are verified from the signature alone, with no database round trip. Revocation
  therefore takes effect at the next refresh; the 15-minute token lifetime is the backstop. Pinned by
  a test so the behaviour cannot drift silently.

Notes from the admin-endpoint pass, recorded so the tradeoffs are not rediscovered:

* **API keys** are stored only as SHA-256 digests under `_settings["auth.api_keys"]` (the `auth.`
  prefix keeps the store out of `GET /api/settings`), returned raw exactly once, verified in
  constant time with revocation and expiry honoured, and **scoped**: an API key reaches the
  collection reads (what codegen needs) and nothing superuser-only. It is accepted as a bearer or
  as `?api_key=`, which the middleware strips before a strict query parser sees it.
* **Cron validation** uses `croner` + `chrono-tz`, which brings the IANA timezone database into
  the binary — a Track L size item, not a free dependency. The cron **scheduler** is still Track I,
  so `next_run_at` is written but nothing fires on it yet.
* **Import rollback** is destructive by design (undoing an import means removing what it added) and
  requires the acting superuser's password. It drops a collection the snapshot lacks with a
  non-CASCADE drop, so one still referenced by a kept collection would refuse, and its
  collection-drop and row-replace phases are not one transaction. Stated rather than hidden.
* The import snapshot is the whole database as **jsonb** in `_import_snapshots`, latest only. That
  is the reference's tradeoff too; a large database pays for it in row size.

### Row-level security: what was decided, and why

The policies, the role switch and the default-deny all work as designed, but three
things about the implementation are worth recording because they are not obvious:

* **No `FORCE ROW LEVEL SECURITY`.** Forcing it makes the schema *owner* subject to policies too,
  which would break the server's own internal reads (the login lookup being the obvious one)
  whenever the node connects as a non-superuser owner. Verified against Postgres: `FORCE` has no
  effect on a superuser, so it would add that risk without adding the guarantee. The owner is the
  service identity — internal work and superusers run as it and bypass policies — while every
  user-facing request runs as `lazypock_anon` or `lazypock_authenticated` through `SET LOCAL ROLE`.
* **Writes do not use `RETURNING`.** Postgres applies the *select* policies to an
  `INSERT ... RETURNING` row, so a caller allowed to create a record but not to see it was refused
  outright — which broke anonymous registration. A write now runs under the request's role, so the
  `WITH CHECK` authorises it, and the stored row is read back as the owner to build the response.
  That is what keeps "the write response is the stored record" true, which is what PocketBase
  clients expect. The write policies still exist as defence in depth for any query that does run
  restricted.
* **`SET ROLE` needs membership, not just existence.** The connecting role must be a member of both
  restricted roles, or be a superuser. The migration grants the roles their privileges but cannot
  know the application's role, so the server probes the switch at boot and warns loudly: a
  connection that cannot switch role is not enforcing anything.

Rules keep their PocketBase three-state meaning (absent denies, `""` is public, a filter becomes
 the predicate) with `manageRule` as a single `for all` policy, and the policies are rebuilt on every
change so a removed rule stops applying.
* `password_hash` and `token_key` are deliberately **absent from the filter context**. Everything in
  that map is both filterable and returned by `fields=*`, so having them there returned the password
  hash to the client on every auth-collection read — a real leak that is now pinned by a test, along
  with a check that naming either column in `fields` or `filter` is refused.

### Kept-out-of-tree code and the Rust server

`lazypock-ts/` is rebuilt against this server's PocketBase-canonical routes and its live
integration test passes. `studio/` is being adapted the same way, and the endpoints it calls — the
collection admin API, `GET /api/export`, logs, crons, API keys and the import-status/rollback pair —
now exist. The old Elixir shape (`/api/{collection}`) is gone; records are addressed as
`/api/collections/{collection}/records`, which the SDK and Studio now use. The Studio's
`adapter-static` output path writes to `build/studio` and moves to `crates/studio-embed/dist` when
that crate lands.

### Roadmap

- **Phase 0 — Foundations** (Track A): workspace, config, errors, logging, CLI, health, CI,
  baseline measurement. *Plus the two spikes below.*
- **Phase 1 — Core API** (B, C, D): migrations, system tables, collections DDL, records CRUD,
  filter/sort/pagination/expand, batch.
- **Phase 2 — Auth & RLS** (E).
- **Phase 3 — Realtime** (F).
- **Phase 4 — Files & images** (G).
- **Phase 5 — Hooks** (H).
- **Phase 6 — Jobs & ops** (I).
- **Phase 7 — Studio + SDK** (J, K).
- **Phase 8 — Polish & release** (L): parity, security hardening, fuzzing, 0.1.0.
- **Phase 9 — Cloud** (M), later.

### Spikes to run before/while locking the design

1. **Driver vs pooler**: `tokio-postgres` + `deadpool-postgres` against (a) direct Postgres and
   (b) PgBouncer transaction pooling / Neon pooled endpoint. Decide the prepared-statement cache
   policy (default off when pooled).
2. **QuickJS + oxc**: binary size, RAM per context, TS-strip and hot-reload time.
3. **NOTIFY throughput/latency** and behavior on the direct endpoint; confirm the 8000-byte
   ids-only design.
4. **Binary size** with embedded Studio (brotli) + rquickjs + oxc against the < 25 MB target.

---

## 18. Risks & things to verify

1. **Prepared statements vs PgBouncer transaction pooling — RESOLVED (spike 1).**
   `tokio-postgres` names every prepared statement (`s{N}`, see
   `tokio-postgres/src/prepare.rs`) and `deadpool-postgres` caches those names across
   checkouts. A pooled endpoint that reassigns the server connection per transaction cannot
   honour either. lazypock therefore requires a **direct** Postgres connection (session
   pooling or the direct endpoint); this is consistent with realtime, which needs the direct
   endpoint for `LISTEN` anyway. `LAZYPOCK_PG_PREPARED_STATEMENTS=off` lets the query layer
   bypass the cross-checkout cache — it does not make transaction pooling safe.
2. **RLS + realtime**: permission checks get expensive with many subscribers; needs batching and
   claims grouping (8.4). Measure early in Phase 3.
3. **RLS superuser/`manageRule` mapping** (6.2) must be tested exhaustively — it is the
   security-critical seam.
4. **Views + RLS**: views must be `security_invoker` or they bypass policies (4.4/6.3).
5. **8000-byte NOTIFY limit** ⇒ design ids-only from the start.
6. **`panic` setting**: `panic="abort"` shrinks the binary but makes in-process recovery
   impossible (one panic drops every live SSE connection). Decide explicitly; the plan's
   graceful-shutdown goal (9) argues for keeping unwind in `realtime`/`api`.
7. **Total DB connections** across instances/tenants: document the formula
   (processes × (pool + 1)); with DB-per-tenant keep pools tiny.
8. **QuickJS lacks Node APIs**: accepted; provide `$http`/`$db` and do not promise npm compat.
9. **Binary size with embedded Studio + oxc + rquickjs**: measure in CI.
10. **Safe schema changes on large tables**: avoid long locks; document expand/contract.
11. **Redis sidecar** (if ever adopted) is a packaging/deployment dependency (8.8).
12. **TLS — RESOLVED.** The pool builds a rustls connector (system trust store) when a URL
    asks for TLS, so `sslmode=require`/`verify-ca`/`verify-full` and the `ssl=require` spelling
    some providers use (Neon) are honoured, and `channel_binding` is dropped because the driver
    does not support it. A live TLS round-trip against Neon succeeds. Two deliberate differences
    from libpq: `require` **verifies the certificate** (libpq only encrypts), which is stricter
    and the safe default; and `prefer` has no plaintext fallback, so it connects unencrypted as
    before rather than silently upgrading. Note for packaging (Track L): rustls' default crypto
    provider is `aws-lc-rs`, a native dependency that must build in the release image.

---

## 19. Open decisions

**All decisions are now confirmed** (section 1). Anything not listed here is considered decided
and can be implemented without further input. New decisions will be added to section 1 as they
are taken.

Resolved: PocketBase-canonical API paths, SSE realtime, RLS from the start, `_`-prefixed table
names, Postgres-first `EventBus` with Redis deferred, EdDSA JWT + JWKS, `panic = "unwind"`,
version `0.1.0`.
