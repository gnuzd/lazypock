# 🦥 LazyPock

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

> **Your whole backend. In one lazy pocket.**

LazyPock is a PocketBase-compatible backend framework built on **Elixir + Phoenix + PostgreSQL**. Define collections in the Studio admin UI, get instant REST API + realtime subscriptions + file storage + auth — all with hooks, rules, and zero boilerplate.

**Status:** Beta — Core backend features working. Auth collections (user JWT, login/refresh, rule integration) ✅. Studio SvelteKit admin UI **ready for beta test**. TypeScript SDK ([lazypock-ts](https://github.com/gnuzd/lazypock-ts), npm: `lazypock`) **ready for beta test**.

---

## Project Structure (Monorepo)

```
LazyPock/
├── core/                  # Elixir backend (Phoenix app)
│   ├── lib/
│   │   ├── lazypock/            # Core library
│   │   │   ├── collections/     # Collection registry + ETS cache
│   │   │   ├── schema/          # DDL engine + type mapper
│   │   │   ├── schemas/         # GenericRecord + FilterCompiler
│   │   │   ├── rules/           # Rule enforcer (PocketBase-compatible)
│   │   │   ├── hooks/           # PocketBase-parity event hooks (Event, Registry, Router)
│   │   │   ├── auth/            # Superuser JWT auth + token
│   │   │   ├── files/           # File storage (local + S3 adapters)
│   │   │   └── realtime/        # Broadcaster (Phoenix PubSub)
│   │   └── lazypock_web/        # Web layer (controllers, channels)
│   │       ├── controllers/     # CollectionController, DynamicController, etc.
│   │       └── channels/        # CollectionChannel, AdminChannel
│   ├── priv/
│   │   ├── hooks/               # User hook modules (*.ex)
│   │   └── static/studio/       # Built Studio SPA
│   ├── test/
│   └── mix.exs
│
├── studio/                # SvelteKit admin UI (SPA)
│   ├── src/
│   │   ├── routes/              # SvelteKit pages
│   │   │   ├── +layout.svelte   # Auth guard + realtime init
│   │   │   ├── login/           # Login page
│   │   │   └── (app)/           # Authenticated pages
│   │   │       └── collections/ # Collection CRUD + records + rules
│   │   └── lib/
│   │       ├── components/      # Reusable Svelte components
│   │       └── client.ts        # LazypockClient singleton
│   └── package.json
│
├── PLAN.md                # Full architecture & development plan
└── README.md
```

> The TypeScript SDK lives in its **own repository** — [github.com/gnuzd/lazypock-ts](https://github.com/gnuzd/lazypock-ts) — and is published to npm as [`lazypock`](https://www.npmjs.com/package/lazypock). It is not part of this monorepo.

---

## What Works Now

| Feature | BE (core) | FE (Studio) | SDK (lazypock-ts) |
| --- | --- | --- | --- |
| 🗄️ **Dynamic Collections** | DDL create/drop/add field. Real Pg tables with real columns. ✅ | Collection CRUD in side pane. Field editor (add/remove/reorder). ✅ | — |
| 🌐 **REST API** | `GET/POST/PATCH/DELETE /api/:collection`. Filter, sort, paginate. ✅ | Record browser with DataTable + dynamic RecordForm. ✅ | `client.collection(name).getList/getFullList/getOne/create/update/delete` ✅ |
| 🔐 **Auth System** | Superuser JWT (setup/login/verify) + **auth collection JWT** (`/:collection/auth-with-password`, `/:collection/auth-refresh`, `/:collection/auth-methods`) + **OAuth2 providers** (`/:collection/auth-with-oauth2`, `/api/oauth2-redirect` popup flow, `_external_auths` linking via Assent — Google/GitHub/generic). Dual token verification in Plug. ✅ | Login page. Auth guard. Token persistence. Auto-redirect. ✅ | `client.login/me/logout`, `authStore` (localStorage-backed) ✅ |
| 🛡️ **Rules** | Three-state (nil=superuser, ""=public, filter). Enforcer for all CRUD + manageRule. Auth user support (non-superusers evaluated against rules). ✅ | Rule editor with lock/unlock per field. `manageRule` field. ✅ | — |
| ⚡ **Realtime** | Phoenix Channels. Broadcaster wired into DynamicController. Rule-enforced join (anonymous allowed on public/rule-based collections). ✅ | Real-time record updates via `client.realtime.subscribe()`. ✅ | `RealtimeService` + PocketBase-style `collection(name).subscribe/unsubscribe`; auto-connects without a token for anon/rule-based access ✅ |
| 📁 **File Storage** | Upload, serve, delete. Local + S3 adapters. ✅ | Upload in record form, image library picker, thumbnails in list/form. ✅ | `files.upload/list/delete`, `getThumbUrl`, `getScaleUrl` ✅ |
| 🪝 **Hooks** | PocketBase-compatible event hooks: `use Lazypock.Hooks.Hook` modules in `priv/hooks/`, `function(e)` + `e.next()` chain, ~70 hooks (App/Record/Collection/BaseModel/Request/Mailer/Realtime), custom API routes via `on_before_serve` + `Router.add`. Legacy Lifecycle/Dispatcher still work (deprecated). ✅ | — | — |
| ⏰ **Cron Jobs** | Persisted `_crons` scheduler: 5/6-field expressions, per-job IANA timezone, SQL / HTTP-webhook / Elixir-hook actions, run-now, pg advisory-lock guarded execution. ✅ | Settings → Cron dashboard: CRUD, enable/disable, run-now, next-run preview, last-run status. ✅ | — |
| 🎨 **Admin Dashboard** | Serves Svelte SPA at `/_/*`. Proxy support in dev. ✅ | Collections sidebar. Record CRUD. Field editor. Rules. Indexes. API key management (Settings → API Keys). Import/Export (Settings → Export/Import Collections). ✅ | — |

---

> **API keys** can be generated from the Studio **Settings → API Keys** dashboard.
> Keys are stored as a SHA-256 hash (raw value shown once at generation) and are
> scoped to collection listing — enough for codegen (`GET /collections`) without
> a login round-trip. See the [lazypock-ts repo](https://github.com/gnuzd/lazypock-ts).

## How to Run

### Docker (easiest — no toolchain needed)

The fastest way to try LazyPock: `docker compose up` starts Postgres + LazyPock
(the **latest release**, built with ImageMagick included) plus example hooks
and a starter migration.

```bash
git clone git@github.com:gnuzd/lazypock.git
cd lazypock/example/docker
docker compose up --build
```

- Server + Studio admin UI: <http://localhost:4000> (`/_/`)
- Superuser (auto-created on first boot): `admin@lazypock.app` / `admin123`

See [example/](example/) for details (example hooks, migrations, and frontend apps).

### Prerequisites

- **Elixir 1.17+** + **Erlang/OTP 26+**
- **PostgreSQL 15+**
- **Node.js 20+** (for Studio admin UI)
- **ImageMagick 7+** (`magick`/`convert`) — required for image thumbnails and
  on-demand scaling (see [File Storage & Thumbnails](#file-storage--thumbnails));
  uploads work without it but no resizing is available
- `zig` and `xz` installed (for Burrito release builds)

### Development

#### 1. Backend (Phoenix)

```bash
cd core

export DATABASE_URL="ecto://postgres:postgres@localhost:5432/lazypock_dev"
mix setup          # Install deps, create DB, run migrations, seed
mix phx.server     # Starts Phoenix on http://localhost:4000
```

#### 2. Studio (SvelteKit Admin UI)

```bash
cd studio

npm install
npm run dev         # Starts Vite dev server on http://localhost:5173
```

> The SvelteKit dev server proxies `/api` requests to the Phoenix backend (port 4000). The Studio is served at `http://localhost:5173/_/`.
>
> For production, the SPA is built into `core/priv/static/studio/` (gitignored —
> generated, not committed). Run `mix assets.build` (or `mix assets.deploy`)
> in `core/` to produce it; the release workflow does this automatically.

#### 3. TypeScript SDK

```bash
git clone git@github.com:gnuzd/lazypock-ts.git
cd lazypock-ts
npm install
npm run build
```

Consumers just install the published package:

```bash
npm install lazypock
```

### First-Time Setup

1. Open Studio at `http://localhost:5173/_/` (or `/_` if serving from Phoenix)
2. You'll be redirected to the login page
3. Click **Setup** to create the first superuser account
4. Login and start managing collections

### Production Release (Burrito single binary)

```bash
cd core
MIX_ENV=prod mix release
# Binary: core/burrito_out/lazypock_macos_silicon
```

#### Required Environment Variables

| Variable | Description | Example |
| --- | --- | --- |
| `DATABASE_URL` | PostgreSQL connection string | `ecto://postgres:postgres@localhost:5432/lazypock_dev` |
| `SECRET_KEY_BASE` | Secret for signing cookies | (generate with `mix phx.gen.secret`) |
| `PHX_HOST` | Public hostname (optional, defaults to `example.com`) | `localhost` |
| `PORT` | HTTP port (optional, defaults to `4000`) | `4000` |
| `POOL_SIZE` | DB connection pool size (optional, defaults to `10`) | `10` |
| `LAZYPOCK_SUPERUSER_EMAIL` | Auto-create superuser on boot | `admin@lazypock.app` |
| `LAZYPOCK_SUPERUSER_PASSWORD` | Auto-create superuser on boot | `your-password` |
| `LAZYPOCK_THUMBNAILS` | Set to `0` to disable thumbnail/scaling generation (see [File Storage & Thumbnails](#file-storage--thumbnails)) | `0` |
| `LAZYPOCK_MIGRATIONS_DIR` | Directory for migrations (default: `~/.lazypock/migrations`) | `/data/lazypock/migrations` |
| `LAZYPOCK_AUTOMIGRATE` | Set to `0` to disable auto-migrate on boot (then use `lazypock migrate`) | `0` |
| `LAZYPOCK_HOOKS_DIR` | Directory for user hooks (default: `~/.lazypock/hooks`) | `/data/lazypock/hooks` |
| `LAZYPOCK_SEEDS_FILE` | Seed file path (default: `~/.lazypock/seeds.exs`) | `/data/lazypock/seeds.exs` |

**Minimal production example:**

```bash
export DATABASE_URL="ecto://postgres:postgres@localhost:5432/lazypock"
export SECRET_KEY_BASE="$(mix phx.gen.secret)"
export PHX_HOST="localhost"
LAZYPOCK_SUPERUSER_EMAIL=admin@example.com LAZYPOCK_SUPERUSER_PASSWORD=changeme \
  ./core/burrito_out/lazypock_macos_silicon
```

The HTTP server is **always started** — no `PHX_SERVER` needed; just run the
binary (or `bin/lazypock start`).

Note: The release uses `RUNTIME_CONFIG=false` (set in `sys.config`), so all configuration is baked in at build time. Environment variables are read by `runtime.exs` via the Elixir config provider during startup.

### Migrations (PocketBase-style)

Two independent migration sources, both tracked in `schema_migrations`:

- **System migrations** (LazyPock's own tables — `_collections`, `_fields`, …)
  ship **inside** the binary (`priv/repo/migrations/`) and are applied
  directly from there on boot. They are never written to a user-visible
  directory.
- **User migrations** live in a **user-writable directory on disk** —
  `~/.lazypock/migrations/` by default (override with
  `LAZYPOCK_MIGRATIONS_DIR`). Only *your* migrations belong there.

System migrations always run before user migrations. Both are applied
automatically on boot (idempotent) and are listed by `lazypock migrations`.

- **To add a collection after a release** (no rebuild needed), drop a
  migration into the migrations dir that uses the collection helpers —
  these register the collection + fields metadata, so it shows up in the
  Studio and is served by the dynamic `/api/:collection` routes:

  ```bash
  # 1. Drop a new migration file into the migrations dir
  cat > ~/.lazypock/migrations/20260915000000_add_posts.exs << 'EOF'
  defmodule Lazypock.Repo.Migrations.AddPosts do
    use Ecto.Migration
    def up do
      Lazypock.Migrations.create_collection("posts",
        type: "base",
        fields: [
          %{"name" => "title", "type" => "text", "required" => true},
          %{"name" => "published", "type" => "bool", "options" => %{"defaultValue" => false}}
        ]
      )
    end
    def down, do: Lazypock.Migrations.drop_collection("posts")
  end
  EOF

  # 2. Apply it (either works)
  lazypock migrate          # apply pending migrations, then exit
  # or restart the server — auto-migrate runs on boot
  ```

  Other helpers: `Lazypock.Migrations.update_collection/2`,
  `Lazypock.Migrations.add_field/2`, `Lazypock.Migrations.drop_field/2`.

  > **Raw `create table` migrations also work** — after every migrate run,
  > any public table that isn't registered in `_collections` is automatically
  > registered as a `base` collection (columns inferred from the Postgres
  > schema), so it shows up in the Studio and is served through
  > `/api/:collection`. Ecto `timestamps()` tables get their `inserted_at`
  > renamed to `created_at` and a missing `updated_at` added, so CRUD works
  > out of the box. Internal `_`-prefixed tables are never touched. Use the
  > helpers above when you want explicit control (rules, indexes, auth type).

- **Check status**: `lazypock migrations`
- **Disable auto-migrate on boot**: `LAZYPOCK_AUTOMIGRATE=0 lazypock`
  (then apply manually with `lazypock migrate`)

### User Hooks (PocketBase `pb_hooks` style)

Hooks live in a **user-writable directory** — `~/.lazypock/hooks/` (override with
`LAZYPOCK_HOOKS_DIR`). They are Elixir `.ex` files that `use Lazypock.Hooks.Hook`,
compiled at runtime on boot and registered alongside the built-in hooks.

```bash
# ~/.lazypock/hooks/posts_hooks.ex
cat > ~/.lazypock/hooks/posts_hooks.ex << 'EOF'
defmodule PostsHooks do
  use Lazypock.Hooks.Hook, collection: "posts"

  # PocketBase: onRecordCreate((e) => { e.record.slug = ...; e.next() })
  def on_record_create(e) do
    slug = e.record["title"] |> to_string() |> String.downcase()
    e = Lazypock.Hooks.Event.put(e, :record, Map.put(e.record, "slug", slug))
    Lazypock.Hooks.Event.next(e)
  end
end
EOF
# restart the server to pick it up (an example post_hooks.ex is copied on first boot)
```

### Seeds

A seed file lives at `~/.lazypock/seeds.exs` (override with `LAZYPOCK_SEEDS_FILE`).
On first boot the bundled `priv/repo/seeds.exs` is copied there and run once after
migrations (tracked in `_seeds_run`). Edit it and re-run anytime:

```bash
lazypock seed            # run the seed file (the CLI re-runs it)
lazypock seed --force    # explicit re-run (same behavior)
```

> Boot-time seeding (on first boot) is idempotent — tracked in `_seeds_run` —
> while the standalone `lazypock seed` command always re-runs the file.

---

## Migrating from PocketBase

Two paths, depending on what you're moving:

### 1. Studio Import/Export (collection-set sync, JSON)

The Studio admin UI can **export** and **re-import** a JSON manifest of your
collections (id/name/type) — the quick way to replicate a collection set
across LazyPock environments or keep it in git:

- **Export** — Settings → **Export Collections**: select collections and download
  `lazypock-schema-<date>.json` (or copy the JSON to clipboard).
- **Import** — Settings → **Import Collections**: paste the JSON or upload the
  file; the page diffs it against your current set (added/removed/changed) and
  can **Delete missing collections** so the target matches the source exactly.

For the full detail — fields, rules, options, hooks, **and records** — use
**Backups**: Settings → **Backups** downloads a full JSON backup
(`GET /api/export`) and restores it from the same page (`POST /api/import`).
Records are restored **upserted by id**, so relations stay intact and
re-restoring the same file never duplicates data.

Both flows are also available from the CLI (run with the release binary):

```bash
lazypock backup                     # write lazypock-backup-<date>.json
lazypock backup /path/to/backup.json
lazypock restore /path/to/backup.json   # restore schema + data
```

### 2. Full PocketBase instance import (data + files, CLI)

Coming from a **running PocketBase app**? LazyPock ships a one-shot importer that
reads a PocketBase `pb_data` directory (SQLite `data.db` + `storage/`) and
recreates everything in Postgres:

```bash
cd core
mix lazypock.import_pocketbase --pb-dir=/path/to/pb_data
# or point directly at the database and storage dirs:
mix lazypock.import_pocketbase --pb-db=/path/to/pb_data/data.db \
  --storage-dir=/path/to/pb_data/storage
```

What gets migrated:

| What | Details |
| --- | --- |
| **Collections** | schema, rules (list/view/create/update/delete/manage), options, custom indexes |
| **Records** | values + original `created`/`updated` timestamps |
| **Relations** | PocketBase record ids are rewritten to **deterministic UUIDv5 ids** so cross-collection references stay intact; `--id-map-file` writes the old→new mapping |
| **Auth collections** | email, bcrypt password hash, `verified`, `emailVisibility` — users log in with their existing passwords |
| **OAuth links** | `_externalAuths` → `_external_auths` (Google/GitHub/generic) |
| **Files** | copied from `pb_data/storage/` into LazyPock's storage |

Requires the `sqlite3` CLI on PATH (used read-only). Dry-run first to preview:

```bash
mix lazypock.import_pocketbase --pb-dir=/path/to/pb_data --dry-run
```

Options: `--pb-dir` (default `pb_data`), `--pb-db`, `--storage-dir`,
`--dry-run`, `--yes` (skip the prompt; also imports records into existing
collections), `--id-map-file` (default `pocketbase_id_map.json`).

---

## File Storage & Thumbnails

Uploaded files are stored on disk under `core/priv/uploads/YYYY/MM/DD/{uuid}.{ext}`
(date-based directories). The `_files` table records metadata (filename, mime type,
size, storage backend, and the collection/record/field the file belongs to).

### Thumbnails & on-demand scaling (requires ImageMagick)

When a **file** or **multi_file** field has **Thumb sizes** configured in the
collection editor (e.g. `50x50, 480x720`), LazyPock generates WebP thumbnails
server-side on upload.

- Thumbnails are stored under `priv/uploads/YYYY/MM/DD/thumbs/`
- Served at `GET /api/files/:id/thumbs/:size`
- The upload response includes a `thumbs` map: `{"50x50": "/api/files/<id>/thumbs/50x50"}`
- The TypeScript SDK exposes `getThumbUrl(baseUrl, fileId, size)` and
  `FileRecord.thumbs`

**Dependency:** all image resizing (thumbnails **and** on-demand scaling) requires
the **ImageMagick** CLI (`magick`/`convert`). Install it on the server:

```bash
# macOS
brew install imagemagick

# Debian/Ubuntu
sudo apt install imagemagick
```

If ImageMagick is **not** installed, uploads still work — the file is stored
normally, but no thumbnails/scaling are available and a one-time warning is
logged. Set `LAZYPOCK_THUMBNAILS=0` to disable image resizing entirely (and
silence the warning).

### On-demand image scaling

Any uploaded **image** can be resized on request (no pre-configuration needed):

```
GET /api/files/:id/scale/:size
```

`:size` is an ImageMagick geometry:

| Size | Behavior |
| --- | --- |
| `100` | width 100px, height auto (keep aspect) |
| `100x` | width 100px (same as `100`) |
| `x100` | height 100px, width auto |
| `100x100` | fit **within** 100×100 box (no upscale) |
| `100x100!` | exact 100×100 (crop/stretch) |

Examples:

```html
<img src="/api/files/<id>/scale/100x100" alt="">
<img src="/api/files/<id>/scale/400x" alt="">
```

- Sizes are validated (max 4 digits per dimension) to prevent abuse.
- Results are **cached** on disk under `priv/uploads/YYYY/MM/DD/thumbs/` — the
  first request generates, subsequent requests are served instantly.
- The TypeScript SDK exposes `getScaleUrl(baseUrl, fileId, size)`.

---

## Quick Preview

Everything below works **today** — run it against the dev setup above. There is
no aspirational syntax here; what isn't built yet is tracked in the
[Development Plan](#development-plan) table.

### REST API

Collections are created in the Studio admin UI (or via the API — see below),
then instantly exposed as REST endpoints:

```bash
# List records (public / rule-allowed collections need no token)
curl http://localhost:4000/api/posts

# Fetch one record
curl http://localhost:4000/api/posts/abc123

# Create a collection (superuser token required)
curl -X POST http://localhost:4000/api/collections \
  -H "Authorization: Bearer <superuser-jwt>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "posts",
    "fields": [
      {"name": "title", "type": "text", "required": true},
      {"name": "body", "type": "text"},
      {"name": "published", "type": "bool"}
    ]
  }'

# Create a record (requires auth unless the collection's createRule allows it)
curl -X POST http://localhost:4000/api/posts \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{"title": "Hello World", "body": "My first post"}'
```

### Auth collections & creating users

Collections come in two types: **base** (plain records) and **auth** (accounts
with an email + write-only password). The built-in **`users`** collection is an
auth collection — like PocketBase, it's a normal (non-system) collection you
can rename, edit, or even delete.

The password field is **write-only**:

- **Hidden** — never returned in API responses, never shown in the Studio,
  excluded from the SDK's default field projection (`hidden: true`).
- **Optional** — accounts may exist without a password (OAuth-only users,
  invite flows), so the DB column is nullable (`required: false`).
- **Hashed** — the server bcrypt-hashes it before storing it.

Create a user exactly like any other record, using `password` as the field
name (PocketBase convention):

```bash
# POST /api/users — public by default (createRule = "")
curl -X POST http://localhost:4000/api/users \
  -H "Content-Type: application/json" \
  -d '{"email": "ada@example.com", "password": "correct-horse-battery"}'

# → 201 {"id": "...", "email": "ada@example.com", "collectionName": "users", ...}
#   (no password / password_hash key in the response — ever)

# Password is optional:
curl -X POST http://localhost:4000/api/users \
  -H "Content-Type: application/json" \
  -d '{"email": "ghost@example.com"}'
```

Login (PocketBase-compatible endpoints):

```bash
# POST /api/users/auth-with-password → { token, record }
curl -X POST http://localhost:4000/api/users/auth-with-password \
  -H "Content-Type: application/json" \
  -d '{"identity": "ada@example.com", "password": "correct-horse-battery"}'

# POST /api/users/auth-refresh → fresh token (Bearer header)
# GET  /api/users/auth-methods  → available auth methods
```

The `password` key is aliased to the actual backing column (`password_hash`)
on write — sending either name works, so the TypeScript SDK below and any
PocketBase SDK behave identically.

```ts
import { LazypockClient } from "lazypock";

const client = new LazypockClient({ baseUrl: "http://localhost:4000/api" });
await client.login("admin@example.com", "password"); // superuser

const posts = await client.collection("posts").getList(1, 30);
const newPost = await client.collection("posts").create({
  title: "Hello World",
  body: "My first post",
});

// Auth collections — create a user (password optional + write-only)
const user = await client.collection("users").create({
  email: "ada@example.com",
  password: "correct-horse-battery",
});
const session = await client.authWithPassword(
  "users",
  "ada@example.com",
  "correct-horse-battery"
);

// Realtime — callback-first, PocketBase-style
client.collection("posts").subscribe((e) => {
  console.log(e.action, e.record);
});
```

### CLI

```bash
lazypock migrate        # apply pending migrations, then exit
lazypock migrations     # show applied/pending migration status
lazypock seed           # run the seed file
lazypock backup         # write a full JSON backup (schema + data) to lazypock-backup-<date>.json
lazypock backup FILE    # ... or to a specific path
lazypock restore FILE   # restore schema + data from a backup JSON (upsert by id)
```

> **Planned** (not available yet): sandboxed runtime hook evaluation and the
> Phase 10 release-polish items — see [Development Plan](#development-plan).

---

## Architecture

```
 Admin Dashboard (SvelteKit SPA)
         │
   ┌─────┼──────┐
   ▼     ▼      ▼
Schema  Hooks  Rules  ← Dynamic engine layer
   │     │      │
   └─────┼──────┘
         ▼
   Generic Data Layer (Ecto)
         │
   ┌─────┼─────┐
   ▼     ▼     ▼
 REST  Realtime Files  ← API layer
   │     │      │
   └─────┼──────┘
         ▼
    PostgreSQL
```

---

## Why Elixir + Phoenix + PostgreSQL?

| PocketBase (Go + SQLite) | LazyPock (Elixir + PostgreSQL) |
| --- | --- |
| Single binary, single file | Single Burrito binary (`mix release`) |
| SQLite | PostgreSQL (transactional DDL, JSONB, real columns) |
| Go hooks (compiled) | Elixir hooks (compiled + declarative + runtime) |
| JS hooks (goja) | Sandboxed runtime hook eval — planned (Phase 10) |
| SSE for realtime | Phoenix Channels (WebSocket + long-polling fallback) |
| Admin UI (Svelte) | SvelteKit SPA (`studio/`) |
| Code-based migrations | Dynamic DDL — zero-code schema changes |

**Elixir's advantages**: Fault-tolerant (BEAM/OTP), massively concurrent (millions of connections), hot code reloads, and macros make dynamic schema generation natural.

---

## Development Plan

See **[PLAN.md](./PLAN.md)** for the full architecture and development plan.

**Current Status:** BE core (Phases 1–7) done. Auth collections (Phase 3 — user JWT, login/refresh, rule integration) ✅. Studio SPA (Phase 8) **ready for beta test**. TypeScript SDK (Phase 9 — [lazypock-ts](https://github.com/gnuzd/lazypock-ts), npm: `lazypock`) **ready for beta test**. Release setup (Phase 10 — Burrito single binary + GitHub Actions release workflow) done.

| Phase | What | Status |
| --- | --- | --- |
| 1 | Foundation (DDL engine, meta tables, registry) | ✅ Complete |
| 2 | Dynamic CRUD API | ✅ Complete |
| 3 | Auth System (superusers + auth collection JWT, login/refresh, rule integration) | ✅ Complete |
| 4 | Rule Engine (Enforcer, three-state, manageRule) | ✅ Complete |
| 5 | Realtime (Phoenix Channels, Broadcaster) | ✅ Complete |
| 6 | File Storage (upload, serve, local + S3) | ✅ Complete |
| 7 | Hook System (PocketBase-parity event hooks: Event + `e.next()` chain, Router for custom API routes) | ✅ Complete |
| 8 | Studio Admin SPA (SvelteKit) | ✅ **Ready for beta test** — collections, records, rules, logs, settings shipped |
| 9 | TypeScript SDK ([lazypock-ts](https://github.com/gnuzd/lazypock-ts), npm: `lazypock`) | ✅ **Ready for beta test** — published on npm, codegen CLI included |
| 10 | Polish & Release | 🔧 **Release setup done** — Burrito single binary + GitHub Actions release workflow; polish items pending |

---

## Key Design Decisions

- **Real columns** — Collections become real PostgreSQL tables with real columns. Full PG power, not a JSONB-only hack.
- **Single tenancy** — One database = one app. Simple, matches PocketBase's mental model.
- **Generic Ecto schema** — One `GenericRecord` module uses `{table, module}` sources to query any table. No runtime module compilation.
- **PocketBase API compatible** — Same filter syntax, same response format, same auth flow. Drop-in replacement for PocketBase SDK users.

---

## Tech Stack

| Layer | Library |
| --- | --- |
| Language | Elixir 1.17+ |
| Framework | Phoenix 1.7+ |
| Database | Ecto + Postgrex + PostgreSQL 15+ |
| Auth | Joken + bcrypt_elixir |
| Admin UI | **SvelteKit SPA** (`studio/`) — built to `core/priv/static/studio/` |
| Image Processing | Vix (libvips) |
| JS SDK | TypeScript — separate repo: [gnuzd/lazypock-ts](https://github.com/gnuzd/lazypock-ts) |
| CSS | Tailwind CSS (Studio), none (core) |

---

## License

[MIT](LICENSE) © 2024-2025 Chris Nguyen (gnuzd)

---

Built with 🦥 by developers who'd rather be napping.
