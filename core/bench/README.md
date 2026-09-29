# Benchmarks

Baseline benchmarks for the LazyPock backend. There were none before; these
establish starting numbers and are intended to be re-run on demand (not part
of CI, since they need a database and are noisy on shared runners).

## Prerequisites

- Dev dependencies installed (`mix deps.get`) — `benchee` is a `:dev`/`:test`
  dependency.
- For DDL benchmarks: a running PostgreSQL and a migrated dev database
  (`mix ecto.create && mix ecto.migrate`).

## Filter compiler (no database)

Parsing is the expensive part of compiling a filter. `FilterCompiler`
memoizes the parsed AST in ETS, so this suite compares **cold** (cache
cleared) vs **warm** compilation to show the memoization benefit.

```bash
cd core
mix bench.filter
```

## DDL + registry throughput (requires a database)

Measures `create_collection` + `drop_collection` serially and at `parallel:
10`, plus `Registry.get/1` (the ETS read on the hot request path).

> Each iteration creates and drops a throwaway `bench_*` collection. Only run
> this against a disposable/dev database.

```bash
cd core
mix ecto.create && mix ecto.migrate
mix bench.ddl
```

## HTTP load (k6)

End-to-end CRUD load against a running server:

```bash
k6 run core/bench/k6/crud_load.js \
  -e BASE_URL=http://localhost:4000/api -e TOKEN=<auth-token>
```

The scenarios ramp `GET /:collection` to 500 VUs, run a constant filtered
list, then ramp `POST /:collection` to 200 RPS. Target thresholds are recorded
as comments (no baseline exists yet); promote them into `options.thresholds`
once a baseline is captured.

## Not yet covered

- **Realtime connection scaling** (subscribe/broadcast fan-out at 1k clients)
  needs a running server and a channel-capable load driver; not included here.
- **`getFullList` / SDK request overhead** — see the `lazypock-ts` repo's
  `bench/` scripts.
