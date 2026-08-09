# 🦥 LazyPock

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

> **Your whole backend. In one lazy pocket.**

LazyPock is a PocketBase-compatible backend framework built on **Elixir + Phoenix + PostgreSQL**. Define collections in the Studio admin UI, get instant REST API + realtime subscriptions + file storage + auth — all with hooks, rules, and zero boilerplate.

**Status:** Beta — Core backend features working. Auth collections (user JWT, login/refresh, rule integration) ✅. Studio SvelteKit admin UI in development.

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
└── lazypock-ts/            # separate repo: github.com/gnuzd/lazypock-ts
    └── (TypeScript SDK → npm: lazypock, kept out of this repo)
│
├── PLAN.md                # Full architecture & development plan
└── README.md
```

---

## What Works Now

| Feature | BE (core) | FE (Studio) | SDK (lazypock-ts) |
|---|---|---|---|
| 🗄️ **Dynamic Collections** | DDL create/drop/add field. Real Pg tables with real columns. ✅ | Collection CRUD in side pane. Field editor (add/remove/reorder). ✅ | — |
| 🌐 **REST API** | `GET/POST/PATCH/DELETE /api/:collection`. Filter, sort, paginate. ✅ | Record browser with DataTable + dynamic RecordForm. ✅ | `client.collection(name).list/getOne/create/update/delete` ✅ |
| 🔐 **Auth System** | Superuser JWT (setup/login/verify) + **auth collection JWT** (`/:collection/auth-with-password`, `/:collection/auth-refresh`, `/:collection/auth-methods`) + **OAuth2 providers** (`/:collection/auth-with-oauth2`, `/api/oauth2-redirect` popup flow, `_external_auths` linking via Assent — Google/GitHub/generic). Dual token verification in Plug. ✅ | Login page. Auth guard. Token persistence. Auto-redirect. ✅ | `client.login/me/logout`, `AuthStore` with localStorage ✅ |
| 🛡️ **Rules** | Three-state (nil=superuser, ""=public, filter). Enforcer for all CRUD + manageRule. Auth user support (non-superusers evaluated against rules). ✅ | Rule editor with lock/unlock per field. `manageRule` field. ✅ | — |
| ⚡ **Realtime** | Phoenix Channels. Broadcaster wired into DynamicController. Rule-enforced join (anonymous allowed on public/rule-based collections). ✅ | Real-time record updates via `client.realtime.subscribe()`. ✅ | `RealtimeService` + PocketBase-style `collection(name).subscribe/unsubscribe`; auto-connects without a token for anon/rule-based access ✅ |
| 📁 **File Storage** | Upload, serve, delete. Local + S3 adapters. ✅ | Upload in record form, image library picker, thumbnails in list/form. ✅ | `files.upload/list/delete`, `getThumbUrl`, `getScaleUrl` ✅ |
| 🪝 **Hooks** | PocketBase-compatible event hooks: `use Lazypock.Hooks.Hook` modules in `priv/hooks/`, `function(e)` + `e.next()` chain, ~70 hooks (App/Record/Collection/BaseModel/Request/Mailer/Realtime), custom API routes via `on_before_serve` + `Router.add`. Legacy Lifecycle/Dispatcher still work (deprecated). ✅ | — | — |
| 🎨 **Admin Dashboard** | Serves Svelte SPA at `/_/*`. Proxy support in dev. ✅ | Collections sidebar. Record CRUD. Field editor. Rules. Indexes. API key management (Settings → API Keys). ✅ | — |

---

> **API keys** can be generated from the Studio **Settings → API Keys** dashboard.
> Keys are stored as a SHA-256 hash (raw value shown once at generation) and are
> scoped to collection listing — enough for codegen (`GET /collections`) without
> a login round-trip. See the [lazypock-ts repo](https://github.com/gnuzd/lazypock-ts).

## How to Run

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

#### 3. TypeScript SDK

```bash
git clone git@github.com:gnuzd/lazypock-ts.git
cd lazypock-ts
npm install
npm run build
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
|---|---|---|
| `DATABASE_URL` | PostgreSQL connection string | `ecto://postgres:postgres@localhost:5432/lazypock_dev` |
| `PHX_SERVER` | Enable the HTTP server (set to `true`) | `true` |
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
export PHX_SERVER=true
export SECRET_KEY_BASE="$(mix phx.gen.secret)"
export PHX_HOST="localhost"
LAZYPOCK_SUPERUSER_EMAIL=admin@example.com LAZYPOCK_SUPERUSER_PASSWORD=changeme \
  ./core/burrito_out/lazypock_macos_silicon
```

Note: The release uses `RUNTIME_CONFIG=false` (set in `sys.config`), so all configuration is baked in at build time. Environment variables are read by `runtime.exs` via the Elixir config provider during startup.

### Migrations (PocketBase-style)

Migrations live in a **user-writable directory on disk** — `~/.lazypock/migrations/`
by default (override with `LAZYPOCK_MIGRATIONS_DIR`). They do NOT live inside
the binary.

- **On first boot**, the bundled migrations (from `priv/repo/migrations/`) are
  copied into that directory (only if they don't already exist — user files are
  never overwritten), then applied automatically.
- **To add a migration after a release** (no rebuild needed):

  ```bash
  # 1. Drop a new migration file into the migrations dir
  cat > ~/.lazypock/migrations/20260915000000_add_custom_table.exs << 'EOF'
  defmodule Lazypock.Repo.Migrations.AddCustomTable do
    use Ecto.Migration
    def up do
      create table(:custom_things) do
        add :name, :text
      end
    end
    def down, do: drop table(:custom_things)
  end
  EOF

  # 2. Apply it (either works)
  lazypock migrate          # apply pending migrations, then exit
  # or restart the server — auto-migrate runs on boot
  ```

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
lazypock seed            # run seeds (idempotent — skips if already run)
lazypock seed --force    # force re-run
```

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

## Quick Preview (Planned API)

```bash
# Create a collection (via Admin UI, or CLI)
mix lazypock.collections.create posts title:text body:text published:bool

# REST API — instantly available
curl https://myapp.fly.dev/api/posts
curl https://myapp.fly.dev/api/posts/abc123
curl -X POST https://myapp.fly.dev/api/posts \
  -H "Content-Type: application/json" \
  -d '{"title": "Hello World", "body": "My first post"}'

# Realtime (JavaScript)
import LazyPock from "lazypock";
const client = new LazyPock("https://myapp.fly.dev");
client.collection("posts").subscribe("*", (e) => {
  console.log(e.action, e.record);
});
```

---

## Architecture

```
 Admin Dashboard (LiveView)
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
|---|---|
| Single binary, single file | Single tarball via `mix release` |
| SQLite | PostgreSQL (transactional DDL, JSONB, real columns) |
| Go hooks (compiled) | Elixir hooks (compiled + declarative + runtime) |
| JS hooks (goja) | Runtime eval with sandboxing |
| SSE for realtime | Phoenix Channels (WebSocket + long-polling fallback) |
| Admin UI (Svelte) | LiveView (server-rendered, reactive, no JS framework) |
| Code-based migrations | Dynamic DDL — zero-code schema changes |

**Elixir's advantages**: Fault-tolerant (BEAM/OTP), massively concurrent (millions of connections), hot code reloads, and macros make dynamic schema generation natural.

---

## Development Plan

See **[PLAN.md](./PLAN.md)** for the full architecture and development plan.

**Current Status:** BE core (Phases 1–7) done. Auth collections (Phase 3 — user JWT, login/refresh, rule integration) ✅. Studio SPA (Phase 8) active. SDK (Phase 9) shipping.

| Phase | What | Status |
|---|---|---|
| 1 | Foundation (DDL engine, meta tables, registry) | ✅ Complete |
| 2 | Dynamic CRUD API | ✅ Complete |
| 3 | Auth System (superusers + auth collection JWT, login/refresh, rule integration) | ✅ Complete |
| 4 | Rule Engine (Enforcer, three-state, manageRule) | ✅ Complete |
| 5 | Realtime (Phoenix Channels, Broadcaster) | ✅ Complete |
| 6 | File Storage (upload, serve, local + S3) | ✅ Complete |
| 7 | Hook System (PocketBase-parity event hooks: Event + `e.next()` chain, Router for custom API routes) | ✅ Complete |
| 8 | Studio Admin SPA (SvelteKit) | 🚧 In Progress |
| 9 | TypeScript SDK | 🚧 Shipping |
| 10 | Polish & Release | ⏳ Planned |

---

## Key Design Decisions

- **Real columns** — Collections become real PostgreSQL tables with real columns. Full PG power, not a JSONB-only hack.
- **Single tenancy** — One database = one app. Simple, matches PocketBase's mental model.
- **Generic Ecto schema** — One `GenericRecord` module uses `{table, module}` sources to query any table. No runtime module compilation.
- **PocketBase API compatible** — Same filter syntax, same response format, same auth flow. Drop-in replacement for PocketBase SDK users.

---

## Tech Stack

| Layer | Library |
|---|---|
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
