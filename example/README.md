# LazyPock Examples

| Folder | What |
| --- | --- |
| [`docker/`](./docker/) | Run a full LazyPock server out of the box — `docker compose up` (Postgres + latest LazyPock release from git, ImageMagick included, example hooks + migration) |
| [`app/`](./app/) | Frontend example apps (React, Svelte, …) that consume the server |

## Quick start — server

```bash
git clone git@github.com:gnuzd/lazypock.git
cd lazypock/example/docker
docker compose up --build
```

- **Server + Studio admin UI:** <http://localhost:4000> (login at `/_/`)
- **Superuser (auto-created on first boot):** `admin@lazypock.app` / `admin123`
- **REST API:** <http://localhost:4000/api/>...

The `lazypock` image pulls the **latest release** from
[github.com/gnuzd/lazypock/releases](https://github.com/gnuzd/lazypock/releases)
and includes **ImageMagick** for thumbnails/on-demand image scaling. Pin a
specific version with `docker build --build-arg LAZYPOCK_VERSION=0.2.2 .`

## What's included

- **Example hooks** (`docker/hooks/`) — mounted at `LAZYPOCK_HOOKS_DIR`; loaded
  at boot. Try the custom API routes after startup:

  ```bash
  curl http://localhost:4000/api/hello/chris     # → {"message":"Hello chris!"}
  curl http://localhost:4000/api/example/time    # → {"now":"..."}
  ```

  Edit the files and `docker compose restart lazypock` to pick up changes.

- **Example migration** (`docker/migrations/`) — mounted at
  `LAZYPOCK_MIGRATIONS_DIR`; applied automatically on boot. It creates the
  `example_notes` table (check with
  `psql postgres://postgres:postgres@localhost:5432/lazypock -c '\d example_notes'`).
  Add your own files with a later timestamp prefix.

- **Persistent data** — Postgres data and `/data/lazypock` (migrations, seeds,
  hooks, uploads) live in named volumes.
