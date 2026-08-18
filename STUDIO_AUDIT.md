# Studio Audit — LazyPock Admin UI vs. Core API Surface

> Date: 2026-08-16 · Scope: `studio/` (SvelteKit SPA) audited against `core/`'s
> HTTP + realtime API (router.ex, controllers, channels).
> Method: inventory of every core route/channel, inventory of every studio
> page/component and the API calls it makes, then a gap matrix.

Status legend: ✅ complete · 🟡 partial · ❌ missing/stub · ➖ not applicable
(no core API for it)

---

## 1. Core API surface inventory

### 1.1 Public (no auth)

| Route | Controller action | Studio counterpart |
|---|---|---|
| `GET /api/health` | HealthController.index | none (not needed) |
| `GET /api/oauth2-redirect` | AuthController.oauth2_redirect | none (browser redirect target) |
| `GET /api/superusers/check` | SuperUserController.check | ✅ login page (checkSuperuser) |
| `POST /api/superusers/setup` | SuperUserController.setup | ✅ login page (first-run setup) |
| `POST /api/superusers/login` | SuperUserController.login | ✅ login page |
| `POST /api/_superusers/auth-with-password` | SuperUserController.superuser_auth_with_password | 🟡 SDK supports; login page uses `/superusers/login` |
| `GET /api/_superusers/auth-methods` | SuperUserController.superuser_auth_methods | ➖ no UI |
| `POST /api/:collection/auth-with-password` | AuthController.auth_with_password | 🟡 via SDK auth; no per-collection login UI |
| `POST /api/:collection/auth-with-oauth2` | AuthController.auth_with_oauth2 | 🟡 via SDK; no OAuth button UI |
| `GET /api/:collection/auth-methods` | AuthController.auth_methods | ❌ no UI |
| `POST /api/:collection/request-verification` | EmailController | ❌ no UI (only SDK) |
| `POST /api/:collection/confirm-verification` | EmailController | ❌ no UI (only SDK) |
| `POST /api/:collection/request-password-reset` | EmailController | ❌ no UI (only SDK) |
| `POST /api/:collection/confirm-password-reset` | EmailController | ❌ no UI (only SDK) |

### 1.2 Authenticated (token verified, enforcer decides)

| Route | Controller action | Studio counterpart |
|---|---|---|
| `GET /api/superusers/me` | SuperUserController.me | ✅ login page / AppHeader session |
| `GET /api/me` | SuperUserController.me | ✅ same |
| `GET /api/collections` | CollectionController.list | ✅ collectionsStore |
| `POST /api/collections` | CollectionController.create | ✅ CollectionEditor |
| `GET /api/collections/:id` | CollectionController.show | ✅ CollectionEditor (getOne) |
| `PATCH /api/collections/:id` | CollectionController.update | ✅ CollectionEditor |
| `DELETE /api/collections/:id` | CollectionController.delete | ✅ CollectionEditor |
| `GET /api/files` | FileController.index | ❌ no file-browser screen |
| `POST /api/files` | FileController.upload | ✅ RecordForm (file fields) |
| `GET /api/files/:id` | FileController.show | ✅ record table thumbnails |
| `GET /api/files/:id/thumbs/:size` | FileController.show_thumb | ✅ record table thumbnails |
| `GET /api/files/:id/scale/:size` | FileController.show_scaled | ✅ via getThumbUrl/scale |
| `DELETE /api/files/:id` | FileController.delete | 🟡 only implicitly via record delete |
| `GET /api/logs` | LogsController.list | ✅ logs page |
| `GET /api/logs/stats` | LogsController.stats | ✅ logs page |
| `GET /api/logs/collections` | LogsController.collections | ✅ logs page |
| `GET /api/logs/:id` | LogsController.show | ✅ logs page (detail) |
| `DELETE /api/logs` | LogsController.delete_logs | ✅ logs page (clear) |
| `GET /api/settings` | SettingsController.show | ✅ application/mail/files pages |
| `PATCH/PUT /api/settings` | SettingsController.update | ✅ application/mail/files pages |
| `POST /api/settings/refresh-cors` | SettingsController.refresh_cors | ✅ application page |
| `GET /api/settings/api-keys` | SettingsController.list_api_keys | ✅ api-keys page |
| `POST /api/settings/api-keys` | SettingsController.generate_api_key | ✅ api-keys page |
| `DELETE /api/settings/api-keys/:id` | SettingsController.revoke_api_key | ✅ api-keys page |
| `GET/POST /api/settings/api-key` (back-compat) | SettingsController | 🟡 legacy; no dedicated UI |
| `POST /api/sql/query` | SettingsController.sql_query | ✅ SQL console |
| `GET /api/export` | SettingsController.export_all | ✅ export page + backups page |
| `POST /api/import` | SettingsController.import_all | ✅ import page |
| `POST /api/settings/test-email` | SettingsController.send_test_email | ❌ no "send test email" button |
| `POST /api/:collection/auth-refresh` | AuthController.auth_refresh | 🟡 via SDK only |

### 1.3 Dynamic record routes

| Route | Controller | Studio counterpart |
|---|---|---|
| `GET/POST /api/:collection` | DynamicController list/create | ✅ collections page (DataTable) |
| `GET/PATCH/PUT/DELETE /api/:collection/:id` | DynamicController | ✅ RecordSidePane |

### 1.4 Realtime

| Channel / topic | Purpose | Studio counterpart |
|---|---|---|
| `CollectionSocket` + `CollectionChannel` (`collection:topic`) | record create/update/delete events, authorization via view/list rules | ✅ collections page (subscribe) |
| `AdminChannel` | admin events | ❌ no UI consumption |

---

## 2. Studio screens inventory

| Route | Screen | API calls | Status |
|---|---|---|---|
| `/login` | superuser check/setup/login | check, setup, login, me, collections.list | ✅ |
| `/collections` (+ `?collection=`) | collection sidebar + record table + record CRUD | collections.list/getOne/create/update/delete/subscribe; collection(name) getList/create/update/delete/subscribe; file thumbs | ✅ |
| `/collections/new` | new-collection page | (redirects to side pane) | ✅ (thin) |
| `/logs` | request logs + stats + detail + clear | logs, logs/stats, logs/collections, logs/:id, delete logs | ✅ |
| `/settings/application` | app name + CORS origins | settings get/patch, refresh-cors | ✅ |
| `/settings/mail` | SMTP config | settings get/patch (mail.*) | 🟡 **no test-email button** |
| `/settings/files` | storage (local/S3) config | settings get/patch (s3.*) | ✅ |
| `/settings/api-keys` | API key list/create/revoke | settings/api-keys CRUD | ✅ |
| `/settings/backups` | backups | GET /export (manual JSON download) | 🟡 no backup/restore parity |
| `/settings/cron` | cron job CRUD + run-now + next-run preview | crons CRUD, POST /crons/:id (run), POST /crons/validate | ✅ |
| `/settings/export` | export collections | collections.list (client-side JSON) | ✅ |
| `/settings/import` | import collections | collections.list, POST /import | ✅ |
| `/settings/sql` | SQL console | POST /sql/query | ✅ |

### 2.1 Shared components

CollectionEditor (fields/rules/indexes/type), RecordForm (per-type inputs),
RecordSidePane (create/edit/password change/delete), DataTable, FieldSettings,
RuleField, IndexesModal, Modal/SidePane/Dropdown/Tabs/Input/SelectField,
RichEditor (TinyMCE), collectionsStore, createForm, ruleValidator.

---

## 3. Gap matrix — missing / incomplete screens

| # | Gap | Core API available? | Severity | Notes |
|---|---|---|---|---|
| G1 | **OAuth provider configuration UI** — no screen to configure Google/GitHub/etc. (`oauth2.providers` runtime settings key exists, read by `Lazypock.Auth.OAuth2.providers/0`) | ✅ `PATCH /settings` accepts `oauth2` key | High | Admins must hand-edit settings via SQL/API today. Provider secrets also live here. |
| G2 | **Auth collection "Auth" tab** — per-auth-collection options (password auth toggle, OAuth providers enabled for the collection, token duration, email template overrides) | 🟡 partial (collection options object) | High | PocketBase's collection editor has this tab; LazyPock's CollectionEditor has Fields/Rules only. |
| G3 | **`auth-methods` viewer + per-collection login UI** — no UI to inspect or exercise a collection's auth methods (password/oauth providers) | ✅ `GET /:collection/auth-methods` | Medium | Useful for admins verifying auth config. |
| G4 | **Email actions on auth records** — request verification / resend, request password reset, (email change) from the record pane | ✅ EmailController routes | Medium | RecordSidePane has only a direct password change. |
| G5 | **External auths viewer** — see which OAuth providers a user linked (`_external_auths`) | 🟡 dynamic collection `_external_auths` readable via API | Medium | Helps admins debug OAuth sign-in. |
| G6 | **Superuser management** — list/create/delete superusers (core: only setup/me for a single superuser) | ❌ no core endpoints (single superuser model) | Low | Requires core work first. |
| G7 | **Backups screen is not real** — only a manual JSON schema download; no backup list/create/download/restore | ❌ no core backup endpoints (`/export` ≠ backup) | Medium | Requires core backup/restore implementation. |
| G8 | ~~Cron screen is a stub~~ — **done**: persisted `_crons` jobs, scheduler, SQL/HTTP/hook actions, timezone-aware next runs, run-now | ✅ crons CRUD + validate + run endpoints | Low | Replaced the stub with a full dashboard. |
| G9 | **File browser / orphaned-file cleanup** — `GET /files` and `DELETE /files/:id` have no dedicated screen | ✅ core exists | Low | Records UI covers the common case. |
| G10 | **"Send test email" button** — core endpoint exists but mail page never calls it | ✅ `POST /settings/test-email` | Low | Small, cheap win. |
| G11 | **Admin realtime events** — `AdminChannel` has no UI consumer (e.g. settings-change toasts) | ✅ AdminChannel | Low | Nice-to-have. |
| G12 | **Field-type parity** — studio field registry omits `multi_file`, `multi_select`, `datetime`, and labels `geo` as `geoPoint` (core accepts `geo`); `FieldSettings` has no editor for `multi_select`/`datetime` options | ✅ core DDL supports all | Medium | Auth collections can't be fully authored in UI. |

---

## 4. Completed vs missing — summary

**Complete:** collection CRUD + field/rule/index editing, record CRUD + bulk ops,
file upload/thumbnails in records, request logs, app/mail/files settings,
API keys, import/export, SQL console, superuser login/setup, realtime record
subscriptions in the record table.

**Missing / incomplete (actionable without core changes):**
- G1 OAuth provider config screen (High)
- G3 auth-methods viewer (Medium)
- G4 email actions on auth records (Medium)
- G5 external-auths viewer (Medium)
- G10 test-email button (Low)
- G12 field-type parity in the editor (Medium)

**Missing (blocked on core work first):**
- G2 per-auth-collection Auth tab options (core option surface needs defining)
- G6 multi-superuser management (core model is single-superuser)
- G7 real backups (core backup/restore endpoints)
- G11 admin realtime consumption

---

## 5. Proposed completion plan (priority order)

Phase A — **quick wins, no core changes** (days):
1. G10: add "Send test email" to `/settings/mail` (uses existing endpoint).
2. G12: align `fieldTypes.ts` with `TypeMapper.valid_types/0` (add multi_file,
   multi_select, datetime; fix geo name); add option editors where missing.
3. G5: "Linked OAuth accounts" panel in RecordSidePane for auth records
   (query `_external_auths` filtered by record id).

Phase B — **auth admin UX** (no core changes):
4. G1: `/settings/oauth` screen — list/enable/disable providers, edit client
   id/secret/redirect URL per provider (persist via `PATCH /settings` →
   `oauth2.providers`), with a live preview of the auth-methods response.
5. G3: auth-methods viewer per collection (read-only tab in collection editor).
6. G4: verification / password-reset email action buttons on auth records in
   RecordSidePane (call existing EmailController endpoints).

Phase C — **requires core design/scope confirmation**:
7. G2: define the per-auth-collection options schema in core (auth tab data
   model) then build the studio Auth tab.
8. G7: core backup/restore (zip of schema+records+files, list/download/restore
   endpoints) then a real Backups screen.
9. G6: multi-superuser CRUD in core, then a superuser management screen.
10. G11: consume AdminChannel events in studio (e.g. settings-change banner).

**Recommended scope for a first implementation pass:** Phase A (1–3) + Phase B (4–6),
i.e. everything achievable purely against the current core API. Phases C items
each involve new core endpoints and should be scheduled with the core work.

---

## 6. Notes & risks

- `lazypock.types.ts` is regenerated from a schema snapshot
  (`npx lazypock-gen`); new screens should use `client.http.*` for admin-only
  endpoints not in the typed client.
- `manageRule` is already surfaced in CollectionEditor (✅) — rules editing is
  complete for the current rule engine surface.
- The collections sidebar hides system collections by default; `_external_auths`,
  `_otps`, `_mfas`, `_auth_origins` are system tables usable by G5.
