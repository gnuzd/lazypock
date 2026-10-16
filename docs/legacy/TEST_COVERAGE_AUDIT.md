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
- Per-record rule enforcement on file uploads (the `/files` endpoint has no
  record context; uploads are authenticated-but-not-rule-gated).
- `Lazypock.Rules.Enforcer` / `Lazypock.Schemas.FilterCompiler` coverage floors
  are ratcheted at the measured baseline (~88-89%), not the 95% target — raise
  them as coverage improves (see `core/scripts/check_rule_coverage.exs`).

### 4.1 Decided: realtime is subscription-gated, not per-record filtered

Per-record realtime filtering was previously listed here as "design decision to
revisit". It is now a **decided, documented parity behaviour**: a subscription
is authorized once at join time against `listRule`, and every subsequent record
change for the collection is delivered. PocketBase behaves the same way
(a subscription is gated by `listRule`/`viewRule`; filtered per-record
subscriptions are not a supported feature upstream). Pinned by
`test/lazypock_web/channels/collection_socket_test.exs`
("broadcasts are gated by the join, not filtered per record") and noted in
`SECURITY.md`.

View collections are **Model A** (= PocketBase): a view's own List/View rules
are authoritative and independent of the source collections' rules, so a view
can expose rows its source tables deny. Pinned by
`view_collections_controller_test.exs`.

## 5. Access-control hardening pass (2026-09-24)

A focused pass over the rule-enforcement path. New tests:

| File | Focus |
| --- | --- |
| `test/lazypock/rules/enforcer_fail_closed_test.exs` | A rule the enforcer cannot prove it evaluated must deny |
| `test/lazypock/schemas/filter_compiler_property_test.exs` | Property/fuzz coverage (arbitrary bytes, generated filters, placeholder/param parity) |
| `test/lazypock_web/controllers/rule_side_channel_test.exs` | No existence oracle; invalid filter rejected; no schema leakage |

Extras: per-module coverage gate (`core/scripts/check_rule_coverage.exs`) + a
path-filtered, nightly-fuzz CI workflow (`rules.yml`); token tampering/expiry
tests in `auth/token_test.exs`; view-builder adversarial-input tests;
`live`-free realtime join-parity guard.

### 5.1 Bugs found and fixed in this pass

1. **Whitespace-only rule granted public access** — `"   "` compiled to an empty
   clause, which the enforcer read as "no restriction". Now classified as
   invalid and denied.
2. **Filters that emit no SQL granted unconditional access** — e.g.
   `listRule = "'a' ~ 'b'"` (a comparison with no field operand) hit the code
   generator's catch-all and produced an empty clause, i.e. allow. `compile/3`
   now rejects a non-empty filter that emits no SQL.
3. **Dangling boolean operators were silently dropped** — `a = 1 &&` compiled as
   `a = 1` instead of erroring.
4. **Unknown `@request.auth.*` tokens bound to empty string** — so
   `@request.auth.typo = ''` evaluated `'' = ''` (true) and allowed. Now rejected
   (deny).
5. **Any struct got the superuser bypass** — `superuser?(%{__struct__: _})`
   matched every struct; now matches only `%Lazypock.Auth.SuperUser{}`.
6. **`~`/`!~` did not escape LIKE metacharacters** — `name ~ '%'` matched every
   row. Now escapes `%`, `_` and `\` (PocketBase `wrapLikeParams` parity) with
   `ESCAPE '\'`, leaving an explicit unescaped `%` as an author pattern.
7. **`?filter=` was silently ignored when invalid** — a typo'd filter returned
   everything the rule allowed. Now a 400.
8. **Rule-denied record was distinguishable from a missing one** — `GET /:id`
   answered 403 vs 404, an existence oracle. Both now answer 404.
9. **Malformed record id raised a 500** — `GET /api/:collection/not-a-uuid`
   crashed with `DBConnection.EncodeError` before the enforcer ran; now a clean
   404 (`GenericRecord.get/3`/`update`/`delete`).
10. **A bare `'` in a filter crashed the compiler** — `unescape_literal/1`
    sliced with a `-1` length (`FunctionClauseError`); found by fuzzing.
11. **Invalid UTF-8 in a filter value crashed the LIKE builder** —
    `String.to_charlist/1` raises `UnicodeConversionError`; the LIKE helpers are
    now byte-wise. Also found by fuzzing.
12. **Filter length / expression limits** — added PocketBase's 3500-character
    and 200-expression caps.
