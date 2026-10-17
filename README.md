# 🦥 lazypock

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.85%2B-000000?logo=rust)](https://www.rust-lang.org)
[![CI](https://img.shields.io/github/actions/workflow/status/gnuzd/lazypock/ci.yml?branch=main&label=CI&logo=github)](https://github.com/gnuzd/lazypock/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/gnuzd/lazypock?label=release&logo=github&sort=semver)](https://github.com/gnuzd/lazypock/releases)
[![Tests](https://img.shields.io/badge/tests-772%20passing-brightgreen?logo=github)](https://github.com/gnuzd/lazypock/actions/workflows/ci.yml)
[![Image](https://img.shields.io/badge/ghcr.io-lazypock-2496ED?logo=docker&logoColor=white)](https://github.com/gnuzd/lazypock/pkgs/container/lazypock)
[![Docs](https://img.shields.io/badge/docs-lazypock.gnuzd.dev-4B275F?logo=readme&logoColor=white)](https://lazypock.gnuzd.dev/)

📚 **Docs:** [What is Lazypock](https://lazypock.gnuzd.dev/) · [Architecture](https://lazypock.gnuzd.dev/architecture) · [Server Guide](https://lazypock.gnuzd.dev/server) · [TypeScript SDK](https://lazypock.gnuzd.dev/sdk/typescript) · [Backup format](https://lazypock.gnuzd.dev/backup)


> **Your whole backend. In one lazy pocket.**

lazypock is a **self-hosted backend-as-a-service**: define collections in the Studio
admin UI and every one gets a REST API, realtime subscriptions, rules and file
storage, with auth, jobs, mail, logs and backups already in the box and no
boilerplate to write.

It is a **single Rust binary on Postgres**. No sidecar services, no Redis, no
separate worker to deploy: the API, auth, realtime, the hooks runtime, the job
queue and the Studio all ship in one artifact, and stateless nodes scale by
starting more processes.

## Status

`main` is the Rust implementation, at **0.22.x**. It replaced an earlier
Elixir/Phoenix build; the Elixir tree is gone, and so is everything that only
made sense for it.

Implemented: schema/DDL engine, records API, batch, Postgres RLS permissions,
auth (password, sessions with rotation and replay detection, OAuth2, email
verification, password reset, email change, account lockout), realtime over SSE,
file storage with presets and a media library, TypeScript hooks, the job queue
and cron scheduler, the mailer, request logs and audit, settings import/export,
backup and restore, and the embedded Studio.

Not yet: outbound webhooks, the PocketBase **data** importer, MFA/TOTP, the
`$db`/`$http`/`$app` hook bindings, and benchmarks/fuzzing in CI.
[`PLAN.md`](PLAN.md) has the architecture, the tracks and the reasoning.

## Priorities

1. Small binary, low RAM — many instances on a 2 vCPU / 2 GB VPS.
2. Lowest possible API latency.
3. Horizontal scaling with stateless nodes.
4. Solid realtime.

## Architecture at a glance

- **One binary** contains the API, auth, realtime, the hooks runtime, the job
  queue, the mailer, the Studio (embedded), migrations and the CLI.
- **Postgres is the source of truth** and the message bus — data, permissions,
  jobs and realtime (`LISTEN`/`NOTIFY`) all live there, so there is no Redis to
  run.
- **Permissions are Postgres RLS policies**, generated from the Studio's rule
  editor: collection rules compile to `CREATE POLICY`, and each request runs
  under a restricted role with its claims set per request.
- **Realtime is SSE**, not a WebSocket: `/api/realtime`, RLS-filtered per
  subscriber, with `Last-Event-ID` resume from `_rt_log`.
- **Image resizing runs in-process** by default. Point
  `LAZYPOCK_IMAGE_SERVICE_URL` at an external service to move it out, or build
  without the `local-image-codecs` feature to leave the codecs out of the binary
  entirely — so there is no sidecar in the default deployment.
- **Hooks are TypeScript**, hot-reloaded in a sandboxed QuickJS runtime without
  recompiling the server.

Full details: [`PLAN.md`](PLAN.md).

## A PocketBase-compatible API

lazypock is its own product, not a PocketBase clone — it stores your data in real
Postgres tables with real columns and enforces permissions with Postgres RLS,
which PocketBase has no equivalent of.

The **API surface is deliberately PocketBase-canonical**, so existing clients keep
working: records are addressed as
`/api/collections/{collection}/records(/{id})` with the PocketBase list envelope,
and `auth-with-password`, `auth-refresh`, `request-password-reset`,
`request-verification`, `auth-with-oauth2`, `POST /api/batch` and
`/api/files/{collection}/{record}/{filename}` are all there. The TypeScript SDK is
a PocketBase-style client and its live integration test runs against this server.

Where it differs on purpose, it says so rather than hiding it: a failed login
answers **401** where PocketBase answers 400, identifiers are `snake_case`
(`_auth_origins`, not `_authOrigins`), and uploads are images-only today (the
Elixir build accepted any file a field allowed).

## Repository layout

```text
crates/
  app/            # the `lazypock` binary: CLI (serve, migrate, superuser, config, healthcheck)
  core/           # config, errors, identifiers, telemetry
  db/             # pool, migration runner, collection/field model, DDL engine,
                  # schema cache, RLS program generation, records and query building, backup
  api/            # axum router and handlers: records, auth, files, logs, crons, jobs,
                  # settings, API keys, SQL console, realtime SSE, Studio hosting
  auth/           # argon2id, EdDSA access tokens, refresh sessions, JWKS
  files/          # upload validation by magic bytes, local and S3 backends, image presets
  hooks/          # QuickJS runtime, TypeScript stripping, loader and hot reload
  mail/           # SMTP transport, MiniJinja templates, the mail job payload
  realtime/       # change events, `_rt_log`, the EventBus seam, Postgres LISTEN, fan-out
  studio-embed/   # the SvelteKit Studio, embedded in the binary
studio/           # SvelteKit admin SPA (bun) — built into the binary via studio-embed
migrations/       # versioned system SQL
docs/             # docs site (SvelteKit)
```

## Quick start

Requires Rust 1.85+ and a Postgres 15+ instance (Docker is fine). A **direct**
connection is required, not a transaction pooler — see [`PLAN.md`](PLAN.md) §18.1.

```bash
cp .env.example .env         # then point LAZYPOCK_DATABASE_URL at your Postgres
cargo run -- migrate         # create the system tables
cargo run -- config          # show the resolved config (credentials redacted)
cargo run -- serve           # start the API on http://localhost:4000
curl http://localhost:4000/api/health
```

```json
{"code":200,"message":"OK","data":{}}
```

Create the first superuser, then open the Studio at <http://localhost:4000/> to
define your first collection — the collections it declares become real tables and
are exposed over REST immediately.

```bash
LAZYPOCK_SUPERUSER_EMAIL=you@example.com \
  LAZYPOCK_SUPERUSER_PASSWORD=… cargo run -- superuser create
```

There is no default superuser and no default password. A container deployment with
no shell to run that command can set both variables and start the server instead:
the account is created at boot **only while no superuser exists**, so a redeploy
never resets it. The Studio's first-run Setup does the same thing, under the same
lock.

### CLI

```bash
lazypock serve        # start the API server (the default)
lazypock migrate      # apply pending database migrations
lazypock migrate --status   # show which migrations are applied
lazypock superuser create --email you@example.com   # or LAZYPOCK_SUPERUSER_EMAIL
lazypock config       # print the resolved configuration, secrets redacted
lazypock healthcheck  # exit 0 when a running server reports healthy
```

## Development

```bash
cargo test --workspace       # unit + integration; needs Postgres
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

The integration tests create their own randomly named schemas, so one database
serves the whole suite. Point them at it with `LAZYPOCK_TEST_DATABASE_URL`, and
set `LAZYPOCK_REQUIRE_DB_TESTS=1` to turn an unreachable database into a failure
instead of a suite that quietly tests nothing.

## License

MIT — see [`LICENSE`](LICENSE).
