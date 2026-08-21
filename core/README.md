# LazyPock Core — Getting Started

## Prerequisites

- Elixir 1.17+, Erlang/OTP 27+
- Docker (for PostgreSQL)

## Quick Start

```bash
# 1. Start PostgreSQL
cd lazyPock
docker compose up -d

# 2. Start the Phoenix server
cd core
mix phx.server

# 3. In another terminal, create a collection (via IEx or Mix run)
mix run -e '
{:ok, _} = LazyPock.Schema.DDL.create_collection("posts", fields: [
  %{"name" => "title", "type" => "text", "required" => true},
  %{"name" => "body", "type" => "editor"},
  %{"name" => "published", "type" => "bool", "default" => false}
])
IO.puts("Collection created!")
'
```

## REST API

Once the server is running, use any HTTP client:

### Create a record

```bash
curl -X POST http://localhost:4000/api/posts \
  -H "Content-Type: application/json" \
  -d '{"data": {"title": "Hello World", "body": "My first post", "published": true}}'
```

### List records

```bash
curl "http://localhost:4000/api/posts"
```

### List with filters (PocketBase syntax)

```bash
# Title contains "hello" AND published is true
curl "http://localhost:4000/api/posts?filter=title~hello%20%26%26%20published=true"

# Category exact match OR featured
curl "http://localhost:4000/api/posts?filter=category=news%20%7C%7C%20featured=true"

# Greater than, less than
curl "http://localhost:4000/api/posts?filter=views>50"
```

### Pagination & sorting

```bash
curl "http://localhost:4000/api/posts?page=1&perPage=10&sort=-created,title"
```

### Get a single record

```bash
curl "http://localhost:4000/api/posts/REPLACE_WITH_UUID"
```

### Update a record

```bash
curl -X PATCH http://localhost:4000/api/posts/REPLACE_WITH_UUID \
  -H "Content-Type: application/json" \
  -d '{"data": {"title": "Updated Title"}}'
```

### Delete a record

```bash
curl -X DELETE http://localhost:4000/api/posts/REPLACE_WITH_UUID
```

## Filter Syntax Reference

| Expression | SQL Equivalent | Example |
|---|---|---|
| `field=value` | `field = value` | `status=active` |
| `field!=value` | `field != value` | `status!=archived` |
| `field~value` | `field ILIKE '%value%'` | `title~hello` |
| `field!~value` | `field NOT ILIKE '%value%'` | `title!~spam` |
| `field>value` | `field > value` | `views>100` |
| `field>=value` | `field >= value` | `views>=100` |
| `field<value` | `field < value` | `price<50` |
| `field<=value` | `field <= value` | `price<=50` |
| `expr1 && expr2` | AND | `published=true && featured=true` |
| `expr1 \|\| expr2` | OR | `category=news \|\| featured=true` |
| `( ... )` | Grouping | `(a=1 \|\| b=2) && c=3` |
| `!expr` | NOT | `!published` |

## Admin (Programmatic)

### Create a collection

```bash
mix run -e '
LazyPock.Schema.DDL.create_collection("products", fields: [
  %{"name" => "name", "type" => "text", "required" => true},
  %{"name" => "price", "type" => "number"},
  %{"name" => "in_stock", "type" => "bool", "default" => true},
  %{"name" => "tags", "type" => "multi_select", "options" => %{values: ["electronics", "clothing", "food"]}},
])
'
```

### Add a field

```bash
mix run -e '
LazyPock.Schema.DDL.add_field("posts", %{"name" => "slug", "type" => "text", "indexed" => true})
'
```

### Drop a collection

```bash
mix run -e '
LazyPock.Schema.DDL.drop_collection("posts")
'
```

## Using the Elixir API

```elixir
# Insert
{:ok, record} = LazyPock.Schemas.GenericRecord.insert("posts", %{title: "Hello"})

# List all
records = LazyPock.Schemas.GenericRecord.all("posts")

# Get by ID
record = LazyPock.Schemas.GenericRecord.get("posts", "uuid-here")

# Update
updated = LazyPock.Schemas.GenericRecord.update("posts", "uuid-here", %{title: "New"})

# Delete
:ok = LazyPock.Schemas.GenericRecord.delete("posts", "uuid-here")

# Count
count = LazyPock.Schemas.GenericRecord.count("posts")

# Count with filter
{:ok, {sql, params}} = LazyPock.Schemas.FilterCompiler.compile("published=true")
count = LazyPock.Schemas.GenericRecord.count_where("posts", sql, params)
```

## Response Format

List responses follow PocketBase conventions:

```json
{
  "page": 1,
  "perPage": 30,
  "totalItems": 42,
  "totalPages": 2,
  "items": [
    {
      "id": "abc-123",
      "title": "Hello",
      "body": "...",
      "created": "2024-01-15T10:30:00.000Z",
      "updated": "2024-01-15T10:30:00.000Z",
      "collectionId": "col-uuid",
      "collectionName": "posts"
    }
  ]
}
```

## Project Structure

```
lazyPock/
├── core/                          # Phoenix backend
│   ├── lib/
│   │   ├── lazyPock/
│   │   │   ├── collections/      # Collection registry & metadata
│   │   │   ├── schema/           # DDL engine & type mapper
│   │   │   ├── schemas/          # GenericRecord & FilterCompiler
│   │   │   └── ecto/             # Custom Ecto types
│   │   └── lazyPockWeb/
│   │       ├── controllers/      # DynamicController, HealthController
│   │       └── views/            # DynamicView (response formatting)
│   ├── priv/repo/migrations/     # Database migrations
│   └── mix.exs
├── docker-compose.yml            # PostgreSQL
├── PLAN.md                       # Full architecture plan
└── README.md
```

## Migrating from PocketBase

Import an existing PocketBase instance (collections, records, auth users, OAuth
links and files) into LazyPock:

```bash
# Dry run first — prints what would be imported without changing anything
mix lazypock.import_pocketbase --pb-dir=/path/to/pb_data --dry-run

# Real import (prompts for confirmation unless --yes)
mix lazypock.import_pocketbase --pb-dir=/path/to/pb_data --yes

# If your PocketBase data lives elsewhere
mix lazypock.import_pocketbase --pb-db=/path/to/data.db --storage-dir=/path/to/storage
```

What happens:

- **Collections** are recreated with their fields, rules and options
  (PocketBase camelCase field names are normalized to `snake_case`).
- **Records** are imported with their original `created`/`updated` timestamps.
  PocketBase ids (15-char strings) are rewritten to deterministic UUIDv5 ids so
  **relations stay intact** even across collections; the old-id → new-id
  mapping is written to `pocketbase_id_map.json`.
- **Auth collections** keep email + bcrypt password hashes (PocketBase and
  LazyPock both use bcrypt) plus `verified`/`email_visibility`; OAuth
  `_externalAuths` links are migrated to `_external_auths`.
- **Files** are copied from `pb_data/storage/` into LazyPock's storage and file
  field values are rewritten from filenames to LazyPock file ids.

Requires the `sqlite3` CLI on PATH (used read-only). Re-running with `--yes`
imports records into existing collections (useful for re-syncs); re-running a
fresh import is idempotent thanks to `ON CONFLICT (id) DO NOTHING` and the
deterministic ids.

## Releasing (automatic)

Releases are fully automatic via [release-please](https://github.com/googleapis/release-please):

1. Push a change to `main` that touches `core/` with a conventional commit
   message (`fix(...)`, `feat(...)`, `chore(...)`). Release Please opens a
   **release PR** — `chore(main): release core-vX.Y.Z` — with the
   `core/mix.exs` version bump and generated `core/CHANGELOG.md`.
2. **Merge the release PR** (review it first — it's the human gate). That
   merge creates the `vX.Y.Z` GitHub Release + tag, then the same workflow
   run runs the test suite, builds the Burrito binaries (`darwin-arm64` +
   `linux-x86_64`), and attaches them (with checksums) to the release.

There is no manual `workflow_dispatch` step.

### Version selection

- `fix(...)` commits → patch (`0.3.0` → `0.3.1`)
- `feat(...)` commits → minor (`0.3.0` → `0.4.0`)
- a `BREAKING CHANGE:` footer in any commit body → major (`0.3.0` → `1.0.0`)

Only commits that touch `core/` participate — Studio and example-app changes
do not bump the server version.
