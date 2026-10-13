# 🦥 lazypock

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.85%2B-000000?logo=rust)](https://www.rust-lang.org)

> **Your whole backend. In one lazy pocket.**

lazypock is a **PocketBase-compatible** backend framework written in **Rust**, on
**Postgres**. Define collections in the Studio admin UI and get an instant REST API,
realtime subscriptions, file storage and auth — with TypeScript hooks, Postgres RLS
permissions and zero boilerplate.

**Status: early rewrite.** This branch (`feat/rust-rewrite`) replaces an earlier
Elixir/Phoenix implementation with a from-scratch Rust build. See
[`PLAN.md`](PLAN.md) for the architecture, tracks and roadmap, and
[`docs/legacy/`](docs/legacy/) for design notes carried over from the Elixir era.

## Priorities

1. Small binary, low RAM — many instances on a 2 vCPU / 2 GB VPS.
2. Lowest possible API latency.
3. Horizontal scaling with stateless nodes.
4. Solid realtime.

## Architecture at a glance

- **One binary** contains the API, auth, realtime, the hooks runtime, the Studio
  (embedded), migrations and the CLI.
- **Postgres is the source of truth** and the message bus (data, RLS, jobs, outbox,
  realtime via `LISTEN/NOTIFY`).
- **Permissions are Postgres RLS policies**, generated from the Studio rule editor.
- **Image resizing runs in a separate service**, never in the API process.
- **Hooks are TypeScript**, hot-reloaded without recompiling the server.

Full details: [`PLAN.md`](PLAN.md).

## Repository layout

```text
crates/
  app/            # `lazypock` binary: CLI (serve, migrate, superuser, config, healthcheck)
  core/           # config, errors, identifiers, telemetry
  api/            # axum router, error mapping
studio/           # SvelteKit admin SPA (bun) — reference copy, rebuilt against the Rust API
docs/             # docs site; docs/legacy/ holds Elixir-era design notes
```

More crates (`db`, `query`, `auth`, `rls`, `realtime`, `files`, `hooks`, `jobs`,
`mailer`, `admin`, `studio-embed`) are added per the plan.

## Quick start

Requires Rust 1.85+ and a Postgres 15+ instance (Docker is fine).

```bash
cp .env.example .env         # then point DATABASE_URL at your Postgres
cargo run -- migrate          # create the system tables
cargo run -- config           # show the resolved config (credentials redacted)
cargo run -- serve            # start the API on http://localhost:4000
curl http://localhost:4000/api/health
```

```json
{"code":200,"message":"OK","data":{}}
```

### CLI

```bash
lazypock serve        # start the API server (default)
lazypock config       # print resolved configuration, secrets redacted
lazypock healthcheck  # exit 0 when a running server reports healthy
lazypock migrate      # apply pending database migrations
lazypock migrate --status   # show which migrations are applied
lazypock superuser create --email you@example.com   # (planned)
```

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

## License

MIT — see [`LICENSE`](LICENSE).
