# 🦥 LazyPock

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
│   │   │   ├── hooks/           # Hook discovery & dispatcher
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
├── packages/
│   └── lazypock-ts/       # TypeScript SDK → npm: lazypock
│       ├── src/
│       │   ├── index.ts    # LazypockClient class
│       │   ├── auth.ts     # AuthStore, JWT management
│       │   ├── collection.ts # Typed collection CRUD
│       │   └── realtime.ts # Phoenix Channel WebSocket client
│       └── package.json
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
| 🔐 **Auth System** | Superuser JWT (setup/login/verify) + **auth collection JWT** (`/:collection/auth-with-password`, `/:collection/auth-refresh`, `/:collection/auth-methods`). Dual token verification in Plug. ✅ | Login page. Auth guard. Token persistence. Auto-redirect. ✅ | `client.login/me/logout`, `AuthStore` with localStorage ✅ |
| 🛡️ **Rules** | Three-state (nil=superuser, ""=public, filter). Enforcer for all CRUD + manageRule. Auth user support (non-superusers evaluated against rules). ✅ | Rule editor with lock/unlock per field. `manageRule` field. ✅ | — |
| ⚡ **Realtime** | Phoenix Channels. Broadcaster wired into DynamicController. Rule-enforced join. ✅ | Real-time record updates via `client.realtime.subscribe()`. ✅ | `RealtimeService` with Phoenix Channel protocol ✅ |
| 📁 **File Storage** | Upload, serve, delete. Local + S3 adapters. ✅ | — | — (via REST API) |
| 🪝 **Hooks** | File-based Elixir hooks. Lifecycle behavior. Dispatcher wired into controller. ✅ | — | — |
| 🎨 **Admin Dashboard** | Serves Svelte SPA at `/_/*`. Proxy support in dev. ✅ | Collections sidebar. Record CRUD. Field editor. Rules. Indexes. ✅ | — |

---

## How to Run

### Prerequisites

- **Elixir 1.17+** + **Erlang/OTP 26+**
- **PostgreSQL 15+**
- **Node.js 20+** (for Studio admin UI)
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
cd packages/lazypock-ts
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
| 7 | Hook System (Lifecycle, Dispatcher, Registry) | ✅ Complete |
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
| JS SDK | TypeScript (`packages/lazypock-ts/`) |
| CSS | Tailwind CSS (Studio), none (core) |

---

## License

MIT

---

Built with 🦥 by developers who'd rather be napping.
