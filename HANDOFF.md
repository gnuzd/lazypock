# Handoff — sdk-apikey-realtime

Branch: `features/sdk-apikey-realtime`
Based on: `main` (~`3c4ed10 feat: update cors`)

## Goal (from user)

1. Fix @lazypock-ts SDK not matching its docs — realtime exposed like PocketBase.
2. Rename `lazypock-gen` to a name that makes more sense (`lazypock`); it "can't run".
3. Codegen currently needs a token (email+password). Support an **API key** generated from the Settings dashboard.
4. (Added) Realtime should honor rules like PocketBase — allow **non-logged-in users** based on API/list rules, not just logged-in users.

## Decisions confirmed with user (before build)

- **API key identity**: read-only pseudo-superuser (enough for codegen `GET /collections`).
- **CLI rename**: primary bin `lazypock`, keep `lazypock-gen` as deprecated alias.

---

## What's DONE (verified)

### TS SDK — `packages/lazypock-ts`

- `src/collection.ts`: added PocketBase-style realtime
  - `collection(name).subscribe(callback, recordId?) => unsubscribe fn`
  - `collection(name).unsubscribe(recordId?)`
  - New exported types `RealtimeMessage` (`action` + `record`), `RealtimeCallback`.
- `src/realtime.ts`:
  - added `isOpen`, `lastUrl`, `lastToken`, `setUrl()`, and `ensureConnected()` so
    `subscribe()` auto-connects on first use **without needing a token**
    (enables anonymous / rule-based realtime).
- `src/lazypock.ts`:
  - `LazypockClient` constructor now auto-derives the socket URL via `wsUrlFromBaseUrl`
    and passes the `RealtimeService` into each `CollectionService`.
- `src/cli.ts`:
  - new `--api-key <key>` flag + `LAZYPOCK_API_KEY` env.
  - When api-key given, skips `superusers/login` and calls `GET /collections`
    directly with `Authorization: Bearer <key>` (recognised by backend Auth.Plug).
  - Fallback keeps `--email/--password`.
- `package.json`: bin = `{ "lazypock": "./dist/cli.js", "lazypock-gen": "./dist/cli.js" }`.
- `smoke-test.mjs`: added realtime wiring smoke tests.
- **Verified**: `npm run build` ✓, `npm run typecheck` ✓, `npm run smoke` ✓.

### Backend (Elixir) — `core`

- **NEW** `lib/lazypock/settings.ex`
  - Central settings read/write (`_settings.data` JSON).
  - `api_key/0`, `set_api_key/1`, `clear_api_key/0`, `new_api_key/0`
    (prefix `lazypock_` + 24 bytes url-safe base64), `secure_compare/2`.
- `lib/lazypock/auth/plug.ex`
  - New: accepts `Authorization: Bearer <api_key>` — if it matches stored key,
    assigns a pseudo-superuser struct (grants read bypass for codegen).
  - Falls back to normal superuser token → auth-collection user token flow.
- `lib/lazypock_web/controllers/settings_controller.ex`
  - New endpoints gated by `require_superuser!`:
    - `GET  /api/settings/api-key`  → returns `{ has_api_key, api_key(masked) }`
    - `POST /api/settings/api-key`  → generates + returns raw key once
    - `DELETE /api/settings/api-key` → revokes
  - General `GET /settings` now masks `api_key` (never leaks raw key).
- `lib/lazypock_web/router.ex`: registered the three new routes under `:auth`.
- **Tests added** (all passing):
  - `test/lazypock/settings_test.exs` (5 tests)
  - `test/lazypock_web/controllers/settings_api_key_test.exs` (6 tests):
    generate / masked get / revoke / API-key-can-list-collections / wrong-key-403 / unauth-403.
- **Realtime anonymous access**: the existing backend already allows anonymous
  socket connect (`CollectionSocket.connect` → `nil` token ok) and `CollectionChannel.join`
  authorizes by `listRule` (public `""` or `@request.auth.*` anon filters). The missing
  piece was SDK-side, which is now fixed (auto-connect without token).
- **Verified**: `mix compile` ✓, `mix test` → **171 passed** (warnings in
  filter_compiler_test are pre-existing type-spec warnings, not new).

### Dashboard (Svelte) — `studio`

- NEW `src/routes/(app)/settings/api-keys/+page.svelte`
  - Generate / regenerate (shows raw once, auto-copies), revoke, masked display.
- `src/routes/(app)/settings/+layout.svelte` added **API Keys** nav item
  (`/settings/api-keys`, `KeyRound` icon).
- **Verified**: `npx svelte-check --tsconfig ./tsconfig.json` → **0 errors**
  (33 warnings are pre-existing a11y warnings in other files; my page has none).

---

## What's PENDING / TO CHECK (+ open items)

1. **End-to-end codegen with API key (not yet run).**
   - Started `mix phx.server` (health OK on :4000). Login for the *existing* superuser
     in the dev DB is unknown — could not get a token to generate an API key live.
   - To verify manually later:

     ```bash
     # login → token, then:
     curl -X POST localhost:4000/api/settings/api-key -H "Authorization: Bearer <TOKEN>"
     cd packages/lazypock-ts
     npx lazypock --url http://localhost:4000/api --api-key <KEY>
     ```

   - If you don't know the admin password, reset the dev DB:

     ```bash
     cd core && mix ecto.drop && mix ecto.create && mix ecto.migrate
     # then curl -X POST .../api/superusers/setup -d '{"email":"a@b.c","password":"password123"}'
     ```

2. **README/docs not yet updated** (task 5 in plan):
   - `packages/lazypock-ts/README.md` — update CLI name (`lazypock`), add `--api-key`,
     add realtime `collection.subscribe` usage + mention anonymous/rule-based access.
   - `root README.md` — mention API keys in Settings if applicable.
   Not done yet.

3. **Open design questions for you:**
   - API key is currently **read-only pseudo-superuser**. It can `GET /collections`
     (codegen) and, because it sets a superuser struct, it bypasses rules on reads.
     Do you want it scoped strictly to `GET /collections` only? (Safer.) Currently
     it behaves as a superuser for auth-read purposes.
   - The generated key is stored **in plain text** in `_settings.data`. For
     production you may want hashing (store only a hash, verify by hashing input).
     Not implemented.

4. **Realtime client-level convenience**: We added `collection.subscribe/unsubscribe`.
   If you also want `client.realtime.subscribe('collection:posts', cb)` style at the
   client level (PocketBase has `pb.collection.subscribe` too), that already exists
   via `RealtimeService.subscribe` — but not auto-connect convenience on the raw
   service. Decide if needed.

---

## Verification quick commands

```bash
# SDK
cd packages/lazypock-ts && npm run build && npm run typecheck && npm run smoke

# Backend
cd core && mix test                # 171 passing

# Studio
cd studio && npx svelte-check --tsconfig ./tsconfig.json   # 0 errors
```

## Files changed

```
M core/lib/lazypock/auth/plug.ex
A core/lib/lazypock/settings.ex
M core/lib/lazypock_web/controllers/settings_controller.ex
M core/lib/lazypock_web/router.ex
A core/test/lazypock/settings_test.exs
A core/test/lazypock_web/controllers/settings_api_key_test.exs
M packages/lazypock-ts/package.json
M packages/lazypock-ts/src/cli.ts
M packages/lazypock-ts/src/collection.ts
M packages/lazypock-ts/src/index.ts
M packages/lazypock-ts/src/lazypock.ts
M packages/lazypock-ts/src/realtime.ts
M packages/lazypock-ts/smoke-test.mjs
A studio/src/routes/(app)/settings/api-keys/+page.svelte
M studio/src/routes/(app)/settings/+layout.svelte
```

NOTE: There is a stray running dev server in the background (log `/tmp/lazypock_server.log`).
Kill with `pkill -f phx.server` when done.
