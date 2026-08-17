# Test Coverage Audit — LazyPock core

> Date: 2026-08-16 · Method: module-by-module inventory of `core/test/` vs
> `core/lib/`, plus runtime verification (full suite = 264 passing at audit time).
> Baseline: `mix test` **264/264 passing**.

## 1. What is covered today

| Area | Test file(s) | Lines | Assessment |
|---|---|---|---|
| Hooks | `test/lazypock/hooks/hooks_test.exs` (Event, Registry, macro), `router_test.exs` | 286 + 75 | **Good.** Chain order, abort, exceptions, collection filtering, after-work, custom routes. |
| Rules enforcer | `test/lazypock/rules/enforcer_test.exs` | 428 | **Good core coverage** (three-state rules, superuser bypass, manageRule, token resolution, cross-collection isolation). Gaps below. |
| Filter compiler | `test/lazypock/schemas/filter_compiler_test.exs` | 400 | **Good.** |
| Dynamic CRUD controller | `test/lazypock_web/controllers/dynamic_controller_test.exs` | 367 | **Good** for list/show/create/update/delete, fields projection, pagination, auth token. |
| Superuser auth | `superuser_controller_test.exs`, `auth/token_test.exs`, `auth/rate_limiter_test.exs` | 135+119+78 | **Good** for superuser login/me/check, token sign/verify, rate limiting. |
| OAuth2 | `auth/oauth2_test.exs` + `oauth2_flow_test.exs` | 155+213 | **Good** for providers(), authorize URL, session store, external-auth linking, mocked flow. |
| DDL engine | `test/lazypock/schema/ddl_indexes_test.exs` | 191 | **Poor.** Indexes only. Core DDL paths untested. |
| TypeMapper | (none dedicated) | 0 | **Untested** (covered implicitly via ddl tests). |
| Realtime channels | (none) | 0 | **Untested.** Socket connect + channel join auth completely uncovered. |
| Files | `test/lazypock/files/scale_test.exs` | 69 | **Poor.** On-demand scale only. Upload/list/delete/thumbs/controller untested. |
| Settings | `settings_test.exs`, `settings_api_key_test.exs` | 144+148 | **Good** (settings upsert, API keys, test-email). |
| Logs / request logger | `plugs/request_logger_test.exs` | 161 | **Good.** |
| Error JSON / views / plugs | `error_json_test.exs`, `dynamic_view_test.exs`, `custom_routes_test.exs` | 12+63+51 | Adequate. |
| Auth-collection JWT flows | (none) | 0 | **Untested.** `auth-with-password`, `auth-refresh`, `auth-methods` for auth collections have no direct test (only the OAuth flow test exercises auth collections indirectly). |
| Email controller (verify/reset) | (none) | 0 | **Untested.** |

## 2. Gap analysis by risk

### 2.1 Schema / DDL engine — HIGH RISK, LOW COVERAGE
`schema/ddl.ex` (749 lines) drives every collection create/update/delete and the
database schema itself. Only index behaviour is tested. Untested:

- `create_collection/2`: name validation (regex), duplicate-name rejection,
  field validation (duplicate names, invalid types), **default rules** for
  base vs auth collections, table actually created with correct columns/PK,
  metadata persisted (`_collections`, `_fields`).
- `add_field/3`: NOT NULL for required, defaults, indexed/unique index creation,
  invalid name/type errors, PubSub broadcast.
- `drop_field/3`: column + metadata removal.
- `update_collection/2`: rename (table + metadata), **system-collection rename
  protection**, type change, metadata (rules/options) updates, field add/remove/
  reorder, relation option normalization (collectionId → name).
- `drop_collection/1`: table drop, **system-collection protection**,
  unmanaged protection.
- `TypeMapper`: full `to_pg_with_opts` mapping, relation `maxSelect > 1` →
  `TEXT[]`, `default_sql` variants (text/number/bool/date), escaping, quote_ident.

### 2.2 Rule enforcer — MEDIUM RISK, GOOD CORE, SOME GAPS
`rules/enforcer.ex` (292 lines). Untested paths:

- `manageRule` short-circuit when the **action rule is nil** (view/create/update/
  delete — the list variant is tested; mutations are not).
- Invalid/compilable-failing filter rule → denied without crash.
- Rule referencing a record that doesn't exist (evaluated against DB id) → denied.
- `authorize_update` against a record whose id isn't in the DB.
- SQL-injection-ish inputs (`'` in email values) are escaped safely.

### 2.3 Auth flows — HIGH RISK, PARTIAL COVERAGE
- Superuser flows: covered.
- **Auth collection JWT flows: untested end-to-end.** Missing:
  - `POST /:collection/auth-with-password` success (token + record), wrong
    password, missing fields, rate-limit 429, non-auth collection, unknown
    collection.
  - `POST /:collection/auth-refresh` success, 401 unauth, 403 cross-collection.
  - `GET /:collection/auth-methods` password-enabled response.
  - **Rule enforcement on auth-collection tokens** (updateRule/deleteRule
    `id = @request.auth.id` — user can only touch own record).
- Email flows (verification / password reset): untested.

### 2.4 Realtime channel authorization — HIGH RISK, ZERO COVERAGE
`collection_socket.ex` + `collection_channel.ex` authorize every realtime
subscription via `Enforcer.authorize_list`. Untested:

- Socket connect: no token (public), invalid token, superuser token, user token.
- Channel join: public collection (empty listRule), denied (nil listRule),
  user-token filter match/mismatch, unknown collection, record-scoped topics.
- Admin channel join.

### 2.5 File upload/thumbnail pipeline — MEDIUM-HIGH RISK, LOW COVERAGE
- `Store.store/3` (binary + Plug.Upload), metadata persistence.
- `Store.list/1` filters (collection, field, mime prefix, pagination).
- `Store.get/1` not-found; `Store.delete/1` (physical + metadata);
  `Store.delete_by_record/2`.
- Thumbnail generation when ImageMagick present (`thumbs` map written).
- FileController endpoints (upload/show/thumb/delete) — no controller test.

## 3. Prioritized plan (what we wrote)

| # | File | Tests | Coverage added |
|---|---|---|---|
| 1 | `test/lazypock/schema/type_mapper_test.exs` | 17 | TypeMapper unit tests (all types/defaults/escaping) |
| 2 | `test/lazypock/schema/ddl_test.exs` | 26 | create/add/drop/update/drop collection + validations + default rules + system protection |
| 3 | `test/lazypock/rules/enforcer_gaps_test.exs` | 12 | manageRule on mutations, invalid rules, missing records, escaping |
| 4 | `test/lazypock_web/controllers/auth_flow_test.exs` | 14 | auth-with-password, auth-refresh, auth-methods, rule enforcement on user tokens |
| 5 | `test/lazypock_web/channels/collection_socket_test.exs` | 11 | socket connect + channel join authorization |
| 6 | `test/lazypock/files/store_test.exs` | 13 | store/get/list/delete/delete_by_record/thumbs |
| 7 | `test/lazypock_web/controllers/file_controller_test.exs` | 12 | upload/show/thumb/delete endpoints + auth requirements |

**Total: +104 tests (baseline 264 → 369, all passing).**

### 3.1 Bugs found & fixed while writing tests

1. **Hooks registry ordering** — `System.unique_integer([:positive])` is not
   monotonic across schedulers; handler dispatch didn't preserve registration
   order. Fixed with `:erlang.unique_integer([:monotonic])` (registry.ex).
2. **DDL create_collection double-wrapped error** — validation failures returned
   `{:ok, {:error, reason}}`; the controller treated them as success and
   broadcast a malformed message that crashed the Registry GenServer. Fixed with
   `Repo.rollback/1` in the `with` else branch (ddl.ex) + defensive Registry
   handlers (registry.ex).
3. **DDL update_collection silent column drop** — omitting `:fields` (bare
   rename, rules-only PATCH) dropped every column. Field sync now guarded on the
   option being present, matching the `:indexes`/`:rules` pattern (ddl.ex).
4. **Rule enforcer crash on bad rules** — a rule referencing a non-existent
   field raised an unhandled `MatchError` (500) instead of denying. Query errors
   now return `false` (enforcer.ex).
5. **Auth-critical: uuid-column rule comparisons crash** — compiled rule params
   bound to `uuid` columns (e.g. auth default `updateRule`/`deleteRule` =
   `id = @request.auth.id`, or `id = ''` for anonymous) crashed Postgrex's
   encoder. Fixed by inlining params as escaped literals (new
   `FilterCompiler.inline_params/2`, used by enforcer + dynamic controller),
   matching the existing create-rule semantics.
6. **Realtime superuser bypass missing** — CollectionSocket assigned a plain map
   for superuser tokens, so the struct-based enforcer bypass never fired.
   Now assigns a `%Lazypock.Auth.SuperUser{}` struct (collection_socket.ex).
7. **Security: anonymous file operations** — `POST /files` accepted anonymous
   uploads (disk exhaustion); `GET /files` and `DELETE /files/:id` were open.
   Upload now requires any authenticated identity (keeps user-token SDK uploads
   working); index/delete require superuser (file_controller.ex).
8. **Upload without `file` part** — crashed with `ActionClauseError` (500); now
   returns a clean 400.

### 3.2 Regression coverage locked in

- Rules-only / rename-only collection updates preserve columns.
- Auth-collection default rules enforce self-only update/delete over HTTP.
- User tokens cannot update/delete other users' records; unauthenticated users
  are denied.
- Anonymous file upload/delete/list are rejected.

## 4. Still open (not written in this pass)

- Email controller flows (verification / password reset) — needs mail mocking.
- S3 adapter tests — need minio or stubbing.
- Per-record realtime event filtering (channel broadcasts are not filtered by
  viewRule per record — subscription is listRule-gated only). Design decision
  to revisit.
- Per-record rule enforcement on file uploads (the `/files` endpoint has no
  record context; uploads are authenticated-but-not-rule-gated).
