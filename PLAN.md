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
  - Hand-written Pratt parser → AST → **fully parameterized SQL** (never string-concatenate
    user input).
  - Column names whitelisted from the schema cache.
  - Operators: `= != > >= < <= ~ !~ ?= ?!=` (any-of for multi), `@now`, `@request.auth.id`, ...
- `sort`: `-created,title`.
- `fields`: column selection, nested with expand.
- `expand`: `expand=author,comments.user` → `LEFT JOIN LATERAL` + `jsonb_build_object`/`json_agg`;
  depth capped (≤ 6) and row counts capped.
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
-- the user's query runs here (RLS applies automatically)
COMMIT;
```

- `set_config(..., true)` is transaction-local, so it is **safe with connection pooling**.
  `SET LOCAL ROLE` also resets at commit, so it is safe with PgBouncer transaction pooling.
- The server connects with a dedicated **non-superuser** role that is a member of
  `app_anon`, `app_authenticated`, `app_service`. `app_service` bypasses RLS and is internal.
- Tables are created with `ENABLE ROW LEVEL SECURITY` **and `FORCE ROW LEVEL SECURITY`** so
  the owning role is also subject to policies.
- SQL helpers: `auth.uid()`, `auth.claim('role')` reading `current_setting`.

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
  in `_collections.rules`, and applies `ENABLE` + `FORCE ROW LEVEL SECURITY`.
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
- Revocation: per-user `token_key`; changing it invalidates all old tokens.

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

### Implementation status

| Track | State |
|---|---|
| A. Foundations | done — workspace, config (fail-loud, secrets redacted), errors, ids, telemetry, CLI, `/api/health` |
| B. Data plane | done — pool, advisory-locked + checksummed migration runner, `_`-prefixed system tables, field types/validation, DDL engine, collections repository, `ArcSwap` schema cache. TLS still outstanding (risk 12). |
| C. Query plane | next |
| D. Records API | not started |
| E. Access control | not started |
| F. Realtime | not started |
| G–I. Files, hooks, jobs | not started |
| J–K. Studio, SDK | not started |
| L. Deploy | not started |
| M. Cloud | not started |

Deleting a collection can keep its data (`drop_data = false`); re-declaring it with
`adoptExistingTable: true` attaches to the kept table, and the adoption is refused when the table's
columns do not match the declaration — so metadata can never misdescribe the real schema.

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
12. **TLS is not implemented yet.** The pool refuses `sslmode=require`/`verify-*` rather than
    silently downgrading (`crates/db/src/pool.rs`), so a Postgres reachable only over TLS (for
    example Neon's direct endpoint) is blocked until the rustls connector lands. Tracked as its
    own task; it does not block local/self-hosted development or Phase 1.

---

## 19. Open decisions

**All decisions are now confirmed** (section 1). Anything not listed here is considered decided
and can be implemented without further input. New decisions will be added to section 1 as they
are taken.

Resolved: PocketBase-canonical API paths, SSE realtime, RLS from the start, `_`-prefixed table
names, Postgres-first `EventBus` with Redis deferred, EdDSA JWT + JWKS, `panic = "unwind"`,
version `0.1.0`.
