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
