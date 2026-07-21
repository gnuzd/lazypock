# 🦥 LazyPock

> **Your whole backend. In one lazy pocket.**

LazyPock is a PocketBase-compatible backend framework built on **Elixir + Phoenix + PostgreSQL**. Define collections in an admin UI, get instant REST API + realtime subscriptions + file storage + auth — all with hooks, rules, and zero boilerplate.

**Status:** 🚧 Planning & Architecture — See [PLAN.md](./PLAN.md)

---

## Project Structure (Monorepo)

```
LazyPock/
├── core/                  # Elixir backend (Phoenix app)
│   ├── lib/
│   │   ├── LazyPock/            # Core library
│   │   │   ├── Collections/     # Collection registry + metadata
│   │   │   ├── Schema/          # DDL engine + type mapper
│   │   │   ├── Schemas/         # GenericRecord + dynamic ecto
│   │   │   ├── Rules/           # Rule compiler + enforcer
│   │   │   ├── Hooks/           # Hook system (3 layers)
│   │   │   ├── Auth/            # JWT + OAuth2
│   │   │   ├── Files/           # File storage + thumbnails
│   │   │   └── Realtime/        # Channel broadcaster
│   │   └── LazyPockWeb/         # Web layer (controllers, channels, liveview)
│   ├── priv/
│   │   └── hooks/               # User hook modules (*.ex)
│   ├── test/
│   └── mix.exs
│
├── sdk-js/                # JavaScript/TypeScript SDK → npm: lazypock
│   ├── src/
│   │   ├── client.ts      # LazyPock client class
│   │   ├── auth.ts        # Auth methods
│   │   ├── collection.ts  # CRUD + subscribe
│   │   ├── files.ts       # File upload/download
│   │   └── realtime.ts    # Phoenix channel wrapper
│   ├── test/
│   └── package.json       # "name": "lazypock"
│
├── sdk-dart/              # Dart/Flutter SDK (future)
├── sdk-py/                # Python SDK (future)
├── PLAN.md                # Full architecture & development plan
└── README.md
```

---

## What It Does

| Feature | How |
|---|---|
| 🗄️ **Dynamic Collections** | Create collections in the admin UI → real PostgreSQL tables with real columns. No migrations, no restarts. |
| 🌐 **Auto REST API** | `GET /api/posts`, `POST /api/posts`, etc. Filter, sort, paginate, expand relations — all PocketBase-compatible. |
| 🔐 **Auth Built-In** | Auth collections with email/password, JWT tokens, OAuth2 (Google, GitHub, etc.), and API keys. |
| ⚡ **Realtime** | Phoenix Channels: subscribe to `collection:posts` or `collection:posts:abc123` and get live updates. |
| 🪝 **Hooks** | Three layers: declarative (JSON config), file-based (Elixir modules), runtime (code in admin UI). |
| 🛡️ **Rules** | PocketBase-style access control: `owner_id = @request.auth.id \|\| @request.auth.role = 'admin'` |
| 📁 **File Storage** | Upload, serve, thumbnail. Local + S3 backends. |
| 🎨 **Admin Dashboard** | Phoenix LiveView dashboard — manage everything without touching code. |
| 📦 **Single Binary** | `mix release` → one tarball. Deploy anywhere. |
| 🔌 **Client SDKs** | Official JS/TS SDK. Dart/Flutter and Python planned. |

---

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

See **[PLAN.md](./PLAN.md)** for the full 10-phase development plan:

| Phase | What | Duration |
|---|---|---|
| 1 | Foundation (scaffold, meta tables, DDL engine) | ~1 week |
| 2 | Dynamic CRUD API | ~1.5 weeks |
| 3 | Auth System | ~1 week |
| 4 | Rule Engine | ~1 week |
| 5 | Realtime (Channels) | ~1 week |
| 6 | File Storage | ~1 week |
| 7 | Hook System | ~1.5 weeks |
| 8 | Admin Dashboard (LiveView) | ~2.5 weeks |
| 9 | Client SDKs | ~1 week |
| 10 | Polish & Release | ~2 weeks |

**Total estimated:** ~13 weeks to v1.0

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
| OAuth2 | Assent |
| Admin UI | Phoenix LiveView 0.20+ |
| Image Processing | Vix (libvips) |
| JS SDK | TypeScript (in `sdk-js/`) |
| Dart SDK | Dart (in `sdk-dart/`, future) |
| Python SDK | Python (in `sdk-py/`, future) |
| CSS | Tailwind CSS (optional) |

---

## License

MIT

---

Built with 🦥 by developers who'd rather be napping.
