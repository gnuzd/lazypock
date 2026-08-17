# LazyPock — Production-Readiness Plan (Priority 1)

> Scope: Priority 1 only (items 1–4). Priority 2 (DX) and Priority 3 (longer-term)
> are NOT started until the user confirms. This file tracks scope, decisions, and
> status for each item; per-item details live in the referenced artifacts.

Status legend: 🔲 planned · 🔧 in progress · ✅ done · ⛔ blocked (needs user decision)

---

## 0. Baseline → Final state

- `mix test` at start: **263/264 passing, 1 failure** (hooks registry ordering).
- `mix test` at end: **373 passing, 0 failures** (3 consecutive clean runs),
  `mix compile --warnings-as-errors` clean.
- +110 tests written; **8 production bugs found & fixed** (see
  TEST_COVERAGE_AUDIT.md §3.1): hooks registry ordering, DDL double-wrapped
  errors + Registry crash, DDL silent column drop on partial updates,
  enforcer crash on bad rules, uuid-column rule comparison crash (auth
  default rules!), realtime superuser bypass missing, anonymous file
  upload/delete/list, missing-file upload crash.
- Postgres via Docker (`lazypock-postgres`, port 5432). Elixir 1.20.3.

## 1. Studio audit vs core API surface — ✅ (audit delivered; implementation pending user sign-off)

Deliverable: `STUDIO_AUDIT.md` (repo root) — full endpoint→screen gap matrix,
missing screens G1–G12, prioritized completion plan (Phase A quick wins,
Phase B auth-admin UX, Phase C core-blocked). No screens implemented yet;
awaiting user confirmation on scope (recommend Phase A + B first).

## 2. Test coverage audit + missing ExUnit tests — 🔲

Deliverable: coverage audit (what exists per module) + new tests, prioritized:

1. **Schema/DDL** — `test/lazypock/schema/` only has `ddl_indexes_test.exs`;
   `schema/ddl.ex` (create/drop/alter tables, type mapping, defaults, PKs,
   system collection protection) is largely untested.
2. **Rules/enforcer** — `enforcer_test.exs` exists (428 lines); audit what rule
   kinds / collection types / error paths are covered, fill gaps.
3. **Auth** — superuser controller tests exist; auth collection JWT flow
   (auth-with-password, refresh, rule integration) coverage to be audited;
   OAuth2 has `oauth2_test.exs` + `oauth2_flow_test.exs` — check Assent-mocked
   paths.
4. **Realtime channel authorization** — collection_socket/channel auth (token →
   subscription rules) has **no dedicated test file**; high priority.
5. **File upload/thumbnail pipeline** — only `files/scale_test.exs` exists;
   upload validation, MIME checks, thumb generation, S3/local adapters untested.

## 3. Hook sandboxing review — ✅ (documented + quick-win guardrails implemented)

Deliverable: `HOOKS_SECURITY.md` (repo root) — trust model (hooks are
same-privilege code compiled into the VM; user hooks compile at runtime from
the user-writable `~/.lazypock/hooks/` dir), capability inventory, existing
guardrails (exception catching, chain abort, boot-time registration),
attack scenarios, and hardening ladder (boot warning → disable env var →
per-handler timeouts → deny-list → separate OS process).

Implemented guardrails:
- `LAZYPOCK_DISABLE_HOOKS=1` env var — skips user-hook loading AND built-in
  discovery (user.ex `disabled?/0` + load!/discover! checks, boot warning).
- Boot warning when hooks load, stating full-privilege execution.
- Test: `test/lazypock/hooks/disable_test.exs`.

NOT implemented (recommended next, needs scope confirmation): per-handler
request-path timeouts; compile-time deny-list (System.halt/:os.cmd/Port).

## 4. PocketBase migration tooling — ✅ (implemented + tested)

Deliverables:
- `core/lib/lazypock/pocketbase/importer.ex` — import engine (sqlite3 CLI
  read-only, zero new deps): collections (schema/rules/options with
  camelCase→snake_case field normalization), records (preserved timestamps,
  deterministic UUIDv5 id mapping so relations survive), auth collections
  (bcrypt hashes, verified/email_visibility columns ensured), `_externalAuths`
  → `_external_auths`, files copied from `pb_data/storage/` with field values
  rewritten from filenames to LazyPock file ids.
- `core/lib/mix/tasks/lazypock.import_pocketbase.ex` — CLI:
  `mix lazypock.import_pocketbase --pb-dir=... [--pb-db] [--storage-dir]
  [--dry-run] [--yes] [--id-map-file]`.
- `core/test/lazypock/pocketbase/importer_test.exs` — end-to-end test with a
  synthetic PocketBase SQLite db (collections, records, relations, auth,
  external auths, files) + dry-run test.
- Docs: core/README.md "Migrating from PocketBase" section.

Design decisions (open for review):
- ID strategy: PB 15-char ids → UUIDv5(collection#id); relations rewritten via
  a pre-built full map (deterministic, order-independent).
- sqlite3 CLI over exqlite dep (zero new dependencies; read-only; `-json`).
- Field names normalized to snake_case (LazyPock DDL requires lowercase);
  auth system columns (verified/email_visibility) ensured post-create.
- `--yes` adopts existing collections for re-sync (records merged via
  `ON CONFLICT (id) DO NOTHING`).
- Not handled: PB view collections, PB casts/`@`-filters import, index
  expressions (mapped to options but not recreated), file thumbs regeneration.


---

## Process

- Each item: update this file's status, update relevant files, run `mix test`
  (and studio build/typecheck where relevant), summarize changes + open items.
- Ask user before any large change or scope expansion.
