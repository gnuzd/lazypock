# LazyPock — Architecture & Development Plan

> **Your whole backend. In one lazy pocket.** 🦥👖

**Status:** Core backend (Phases 1–7) complete — DDL engine, dynamic CRUD with relation expansion, auth collections (superuser + user JWT, login/refresh, rule integration), rule engine with Studio syntax validation, realtime, file storage (local + S3), and a PocketBase-parity event hook system (`use Lazypock.Hooks.Hook` modules, `e.next()` chain, custom API routes via `on_before_serve`). Studio SPA (Phase 8) in active development — collections, records, rules, logs, settings shipped. SDK (Phase 9) published as `lazypock-ts` v0.1.1 (npm) with full type safety via codegen CLI.

LazyPock is a PocketBase-compatible backend framework built on **Elixir + Phoenix + PostgreSQL**.
Define collections in an admin UI, get instant REST API + realtime subscriptions + file storage + auth — all with hooks, rules, and zero boilerplate.

### Monorepo Structure

```
LazyPock/
├── core/                  # Elixir backend (Phoenix app)
│   ├── lib/
│   │   ├── lazypock/            # Core library
│   │   │   ├── collections/     # Collection registry + ETS cache
│   │   │   ├── schema/          # DDL engine + type mapper
│   │   │   ├── schemas/         # GenericRecord + FilterCompiler
│   │   │   ├── rules/           # Rule enforcer
│   │   │   ├── hooks/           # PocketBase-parity event hooks (Event, Registry, Router)
│   │   │   ├── auth/            # Superuser JWT auth
│   │   │   ├── files/           # File storage (local + S3)
│   │   │   └── realtime/        # Channel broadcaster
│   │   └── lazypock_web/        # Controllers, channels, router
│   ├── priv/hooks/              # User hook modules (*.ex)
│   ├── test/
│   └── mix.exs
│
├── studio/                # SvelteKit admin SPA
│   ├── src/
│   │   ├── routes/              # Login, collections, layout
│   │   └── lib/components/      # Reusable Svelte components
│   └── package.json
│
└── lazypock-ts/            # separate repo: github.com/gnuzd/lazypock-ts
│
├── PLAN.md
└── README.md
```

Note: `core/` is the root Phoenix project. All CLI commands (e.g. `mix lazypock.new`) refer to
scaffolding a new app FROM this core, not within the monorepo itself.

---

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Phase 1 — Foundation (Scaffold + Meta Tables + DDL Engine)](#phase-1--foundation)
3. [Phase 2 — Dynamic CRUD API](#phase-2--dynamic-crud-api)
4. [Phase 3 — Auth System](#phase-3--auth-system)
5. [Phase 4 — Rule Engine](#phase-4--rule-engine)
6. [Phase 5 — Realtime (Channels)](#phase-5--realtime)
7. [Phase 6 — File Storage](#phase-6--file-storage)
8. [Phase 7 — Hook System](#phase-7--hook-system)
9. [Phase 8 — Studio Admin SPA (SvelteKit)](#phase-8--studio-admin-spa-sveltekit)
10. [Phase 9 — Client SDKs](#phase-9--client-sdks)
11. [Phase 10 — Polish & Release](#phase-10--polish--release)

---

## Architecture Overview

```
                     ┌─────────────────────────────────┐
                     │     Studio (SvelteKit SPA)        │
                     │  Collections │ Records │ Rules     │
                     │  Fields │ Indexes │ Auth            │
                     └──────────────────┬──────────────────┘
                                        │
                     ┌──────────────────┼──────────────────┐
                     │                  ▼                  │
                     │          Phoenix Backend            │
                     │   Controllers → Auth Plug           │
                     └──────────────────┬──────────────────┘
                                        │
             ┌──────────────────────────┼──────────────────────────┐
             ▼                          ▼                          ▼
   ┌─────────────────┐    ┌──────────────────┐    ┌───────────────────┐
   │  DDL Engine     │    │  Hook Dispatcher │    │  Rule Enforcer    │
   │  • create/drop  │    │  • Lifecycle      │    │  • Three-state     │
   │  • TypeMapper   │    │  • Registry       │    │  • manageRule      │
   │  • Metadata CRUD│    │  • Pre/after hooks│    │  • SQL WHERE gen   │
   └────────┬────────┘    └────────┬─────────┘    └────────┬──────────┘
            │                      │                       │
            └──────────────────────┼───────────────────────┘
                                   │
                   ┌───────────────▼──────────────────┐
                   │    Generic Data Layer            │
                   │  GenericRecord (SQL)             │
                   │  DynamicController (CRUD)        │
                   │  Realtime Broadcaster            │
                   │  Broadcaster → Phoenix Channels  │
                   └───────────────┬──────────────────┘
                                   │
                   ┌───────────────▼──────────────────┐
                   │    PostgreSQL                    │
                   │  • User tables (real cols)       │
                   │  • _collections, _fields         │
                   │  • _files, _superusers           │
                   └──────────────────────────────────┘
```

### Key Design Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Schema approach | Real columns + `{table, GenericRecord}` source | Full Postgres power, no runtime module compilation |
| Tenancy | Single tenant (one DB = one app) | Simplicity. Matches PocketBase's "one SQLite file = one app" |
| Rule engine | PocketBase-style DSL → Ecto dynamic queries | Declarative, compilable to SQL for performance |
| Hooks | 3 layers: Declarative (JSON), File-based (.ex), Runtime (sandboxed eval) | Progressive power: zero-code → full Elixir |
| Auth | Custom JWT provider (dual token: superuser + auth collection) | Superuser tokens + per-collection auth user tokens, both verified in Plug |
| Realtime | Phoenix Channels + PubSub | Native to Phoenix, proven at scale |
| File storage | Waffle + custom adapter (local/S3) | Pluggable backends |
| Releases | `mix release` → single tarball | "One binary" experience like PocketBase |

---

## Phase 1 — Foundation

**Goal:** Scaffold the Phoenix app, create meta tables, build the DDL engine that creates/drops real PostgreSQL tables dynamically.

**Duration:** ~1 week

### 1.1 Project Scaffold

```bash
mix phx.new lazypock --database postgres --no-mailer --no-dashboard --no-gettext
```

> ℹ️ When developing the LazyPock project itself, the Phoenix app lives in `core/`.
> End users run `mix lazypock.new my-app` to scaffold their own project.

- Phoenix 1.7+, Elixir 1.17+, Erlang/OTP 27+
- Ecto + Postgrex for PostgreSQL
- Remove unused generators (`mailer`, `gettext`, default dashboard)
- Configure `config/runtime.exs` for `DATABASE_URL`

### 1.2 Meta Tables (`_collections`, `_fields`)

Create migrations for the system tables that store collection/field metadata:

#### `_collections`

| Column | Type | Description |
|---|---|---|
| `id` | UUID PK | |
| `name` | TEXT UNIQUE NOT NULL | Becomes the PostgreSQL table name (e.g. "posts") |
| `type` | TEXT NOT NULL DEFAULT 'base' | `base` or `auth` |
| `schema` | JSONB NOT NULL DEFAULT '[]' | Cached snapshot of all fields (denormalized for fast reads) |
| `rules` | JSONB NOT NULL DEFAULT '{}' | `{listRule, viewRule, createRule, updateRule, deleteRule}` |
| `options` | JSONB NOT NULL DEFAULT '{}' | `{timestamps, soft_delete, ...}` |
| `hooks` | JSONB NOT NULL DEFAULT '{}' | Declarative hook configs |
| `managed` | BOOLEAN NOT NULL DEFAULT TRUE | If false, table is external (never DROP) |
| `created_at` | TIMESTAMPTZ | |
| `updated_at` | TIMESTAMPTZ | |

#### `_fields`

| Column | Type | Description |
|---|---|---|
| `id` | UUID PK | |
| `collection_id` | UUID FK → _collections | |
| `name` | TEXT NOT NULL | Column name (e.g. "title") |
| `type` | TEXT NOT NULL | `text`, `number`, `bool`, `email`, `url`, `date`, `select`, `multi_select`, `file`, `multi_file`, `json`, `relation`, `editor`, `password` |
| `required` | BOOLEAN DEFAULT FALSE | |
| `unique` | BOOLEAN DEFAULT FALSE | |
| `default_value` | JSONB | Type-cast default |
| `options` | JSONB DEFAULT '{}' | `{max_length, values[], max_size, ...}` |
| `indexed` | BOOLEAN DEFAULT FALSE | |
| `hidden` | BOOLEAN DEFAULT FALSE | Hidden from API responses (e.g. password_hash) |
| `system` | BOOLEAN DEFAULT FALSE | System-managed field (not user-editable) |
| `sort_order` | INTEGER DEFAULT 0 | |
| `created_at` | TIMESTAMPTZ | |
| `updated_at` | TIMESTAMPTZ | |

Unique constraint: `(collection_id, name)`

### 1.3 PostgreSQL Type Mapper

```elixir
defmodule LazyPock.Schema.TypeMapper do
  @pg_types %{
    "text"          => "TEXT",
    "number"        => "NUMERIC",
    "bool"          => "BOOLEAN",
    "email"         => "TEXT",
    "url"           => "TEXT",
    "date"          => "TIMESTAMPTZ",
    "select"        => "TEXT",
    "multi_select"  => "TEXT[]",
    "file"          => "TEXT",
    "multi_file"    => "TEXT[]",
    "json"          => "JSONB",
    "relation"      => "UUID",
    "editor"        => "TEXT",
    "password"      => "TEXT",
  }

  @ecto_types %{
    "text"          => :string,
    "number"        => :decimal,
    "bool"          => :boolean,
    "email"         => :string,
    "url"           => :string,
    "date"          => :utc_datetime_usec,
    "select"        => :string,
    "multi_select"  => {:array, :string},
    "file"          => :string,
    "multi_file"    => {:array, :string},
    "json"          => :map,
    "relation"      => :binary_id,
    "editor"        => :string,
    "password"      => :string,
  }

  def to_pg(type), do: Map.fetch!(@pg_types, type)
  def to_ecto(type), do: Map.fetch!(@ecto_types, type)
end
```

### 1.4 DDL Engine

```elixir
defmodule LazyPock.Schema.DDL do
  @moduledoc """
  Executes DDL operations safely using PostgreSQL's transactional DDL.
  All operations run in a transaction — if anything fails, everything rolls back.
  """

  alias LazyPock.Repo
  alias LazyPock.Schema.TypeMapper

  @doc """
  Creates a new collection table and stores metadata.
  Uses advisory lock to prevent concurrent schema changes to the same collection.
  """
  def create_collection(name, type, fields) do
    Repo.transaction(fn ->
      # 1. Validate name (alphanumeric + underscore, not a reserved word)
      :ok = validate_collection_name!(name)

      # 2. Create metadata record
      collection = %LazyPock.Collections.Collection{
        name: name,
        type: type,
        schema: fields,
        managed: true
      }
      |> Repo.insert!()

      # 3. Build CREATE TABLE SQL
      columns = Enum.map(fields, &column_def/1)
      sql = """
      CREATE TABLE #{quote_ident(name)} (
        id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
        #{Enum.join(columns, ",\n        ")},
        created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
        updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
      )
      """

      # 4. Execute DDL
      Ecto.Adapters.SQL.query!(Repo, sql, [])

      # 5. Create indexes for unique/indexed fields
      Enum.each(fields, fn field ->
        if field["indexed"], do: create_index(name, field["name"])
        if field["unique"], do: create_unique_index(name, field["name"])
      end)

      # 6. Store field metadata
      Enum.each(fields, fn field_def ->
        %LazyPock.Collections.Field{
          collection_id: collection.id,
          name: field_def["name"],
          type: field_def["type"],
          required: Map.get(field_def, "required", false),
          unique: Map.get(field_def, "unique", false),
          default_value: field_def["default"],
          options: Map.get(field_def, "options", %{}),
          indexed: Map.get(field_def, "indexed", false),
          sort_order: Map.get(field_def, "sort_order", 0)
        }
        |> Repo.insert!()
      end)

      # 7. Broadcast to cluster
      LazyPock.PubSub.broadcast!("schema", {:collection_created, collection})

      collection
    end)
  end

  @doc """
  Adds a column to an existing collection table.
  PG 11+: ADD COLUMN is instant for nullable columns without defaults.
  """
  def add_field(collection_name, field_def) do
    Repo.transaction(fn ->
      # 1. Validate field doesn't already exist
      :ok = validate_field_name!(collection_name, field_def["name"])

      # 2. Execute ALTER TABLE
      sql = """
      ALTER TABLE #{quote_ident(collection_name)}
      ADD COLUMN #{quote_ident(field_def["name"])}
      #{TypeMapper.to_pg(field_def["type"])}
      #{if field_def["required"], do: "NOT NULL", else: ""}
      #{build_default(field_def)}
      """
      Ecto.Adapters.SQL.query!(Repo, sql, [])

      # 3. Create index if needed
      if field_def["indexed"], do: create_index(collection_name, field_def["name"])

      # 4. Store metadata
      # 5. Update cached schema in _collections
      # 6. Broadcast
      :ok
    end)
  end

  def drop_field(collection_name, field_name) do
    Repo.transaction(fn ->
      Ecto.Adapters.SQL.query!(Repo,
        "ALTER TABLE #{quote_ident(collection_name)} DROP COLUMN IF EXISTS #{quote_ident(field_name)}", [])
      # Delete metadata, update cache, broadcast
      :ok
    end)
  end

  def drop_collection(name) do
    Repo.transaction(fn ->
      collection = Repo.get_by!(LazyPock.Collections.Collection, name: name)
      if collection.managed do
        Ecto.Adapters.SQL.query!(Repo, "DROP TABLE IF EXISTS #{quote_ident(name)} CASCADE", [])
        Repo.delete!(collection)
        LazyPock.PubSub.broadcast!("schema", {:collection_deleted, name})
      else
        {:error, :not_managed}
      end
    end)
  end

  # --- Private helpers ---
  defp column_def(field) do
    "#{quote_ident(field["name"])} #{TypeMapper.to_pg(field["type"])}#{if field["required"], do: " NOT NULL", else: ""}"
  end

  defp quote_ident(name), do: "\"#{name}\""
  defp build_default(%{"default" => nil}), do: ""
  defp build_default(%{"default" => val}), do: "DEFAULT #{escape_value(val)}"
  defp build_default(_), do: ""
end
```

### 1.5 Collection Registry (GenServer + ETS)

```elixir
defmodule LazyPock.Collections.Registry do
  @moduledoc """
  In-memory cache of all collections and their field definitions.
  Backed by ETS for fast reads. Updated via PubSub on schema changes.
  """

  use GenServer

  @table_name :lazypock_collections

  def start_link(_opts) do
    GenServer.start_link(__MODULE__, %{}, name: __MODULE__)
  end

  @impl true
  def init(_) do
    table = :ets.new(@table_name, [:named_table, :protected, :set, read_concurrency: true])

    # Load all collections from DB into ETS
    Repo.all(LazyPock.Collections.Collection)
    |> Repo.preload(:fields)
    |> Enum.each(fn col ->
      :ets.insert(table, {col.name, col})
    end)

    {:ok, %{table: table}}
  end

  def get(name), do: :ets.lookup(@table_name, name) |> case do
    [{^name, collection}] -> {:ok, collection}
    [] -> {:error, :not_found}
  end

  def list, do: :ets.tab2list(@table_name) |> Enum.map(&elem(&1, 1))

  # Called via PubSub broadcast
  @impl true
  def handle_info({:collection_created, collection}, state) do
    collection = Repo.preload(collection, :fields)
    :ets.insert(state.table, {collection.name, collection})
    {:noreply, state}
  end

  def handle_info({:collection_deleted, name}, state) do
    :ets.delete(state.table, name)
    {:noreply, state}
  end
end
```

### 1.6 GenericRecord — The Universal Ecto Schema

```elixir
defmodule LazyPock.Schemas.GenericRecord do
  @moduledoc """
  A single Ecto schema that can represent ANY collection.
  Uses Ecto's `{table_atom, module}` source to target any table at query time.
  """

  use Ecto.Schema

  @primary_key {:id, :binary_id, autogenerate: true}
  schema "dynamic_table_placeholder" do
    timestamps()
  end

  @doc """
  Returns a query targeting the real table.
  """
  def query(collection_name) when is_binary(collection_name) do
    table = String.to_existing_atom(collection_name)
    from(r in {table, __MODULE__})
  end

  @doc """
  Dynamically build a changeset from collection field metadata.
  """
  def changeset(attrs, collection) do
    types = build_types_map(collection.fields)
    data = %{}

    {data, types}
    |> Ecto.Changeset.cast(sanitize_attrs(attrs, collection.fields), Map.keys(types))
    |> apply_field_validations(collection.fields)
  end

  # ... private helpers for type mapping, validation application
end
```

### 1.7 Deliverables (Phase 1)

- [x] Phoenix project scaffolded with PostgreSQL
- [x] `_collections` and `_fields` migrations + Ecto schemas
- [x] `TypeMapper` module (PG types + Ecto types)
- [x] `DDL` module (create/drop collection, add/drop field, update metadata)
- [x] `CollectionRegistry` GenServer + ETS cache
- [x] `GenericRecord` — SQL-based record CRUD helper
- [x] `FilterCompiler` — PocketBase filter → SQL WHERE
- [ ] Mix task: `mix lazypock.create_collection` (TS codegen CLI shipped in the `lazypock-ts` repo instead)

---

## Phase 2 — Dynamic CRUD API

**Goal:** Generic REST endpoints that auto-discover collections and handle CRUD, filtering, sorting, pagination, and relation expansion.

**Duration:** ~1.5 weeks

### 2.1 Dynamic Router

```elixir
defmodule LazyPockWeb.Router do
  use LazyPockWeb, :router

  # Static routes first (admin UI, auth, health)
  scope "/", LazyPockWeb do
    get "/health", HealthController, :index
  end

  scope "/api", LazyPockWeb do
    # Auth routes (outsourced to auth system later)
    post "/auth/login", AuthController, :login
    post "/auth/register", AuthController, :register
    post "/auth/refresh", AuthController, :refresh

    # File routes
    scope "/files", FilesController do
      get "/:id", :show
      post "/", :upload
      delete "/:id", :delete
    end
  end

  # Dynamic collection routes — MUST be last
  # Matches: GET /api/posts, GET /api/posts/:id, POST /api/posts, etc.
  scope "/api", LazyPockWeb do
    get "/:collection", DynamicController, :list
    get "/:collection/:id", DynamicController, :show
    post "/:collection", DynamicController, :create
    patch "/:collection/:id", DynamicController, :update
    put "/:collection/:id", DynamicController, :update
    delete "/:collection/:id", DynamicController, :delete
  end
end
```

### 2.2 DynamicController

Handles all collection CRUD transparently:

```elixir
defmodule LazyPockWeb.DynamicController do
  use LazyPockWeb, :controller

  alias LazyPock.Collections.Registry
  alias LazyPock.Schemas.GenericRecord
  alias LazyPock.Repo

  def list(conn, %{"collection" => name} = params) do
    with {:ok, collection} <- Registry.get(name) do
      query =
        collection.name
        |> GenericRecord.query()
        |> apply_filters(params, collection)
        |> apply_sort(params, collection)
        |> apply_pagination(params)

      records = Repo.all(query)
      total = Repo.aggregate(GenericRecord.query(collection.name), :count, :id)

      conn
      |> put_resp_header("x-total-count", to_string(total))
      |> render(:list, records: records, collection: collection)
    end
  end

  def show(conn, %{"collection" => name, "id" => id}) do
    with {:ok, collection} <- Registry.get(name),
         record <- GenericRecord.query(name) |> Repo.get!(id) do
      render(conn, :show, record: record, collection: collection)
    end
  end

  def create(conn, %{"collection" => name, "data" => attrs}) do
    with {:ok, collection} <- Registry.get(name),
         changeset <- GenericRecord.changeset(attrs, collection),
         {:ok, record} <- Repo.insert(changeset) do
      LazyPock.PubSub.broadcast!("collection:#{name}", {:created, record})
      conn |> put_status(201) |> render(:show, record: record, collection: collection)
    end
  end

  # update, delete similar...
end
```

### 2.3 Advanced Query Features

Support PocketBase-compatible query parameters:

```
GET /api/posts?filter=(title~'hello'&&created>'2024-01-01')&sort=-created,title&page=1&perPage=20&expand=author,categories&fields=id,title,created
```

| Parameter | Description | Example |
|---|---|---|
| `filter` | PocketBase filter syntax | `(title~'hello'&&created>'2024-01-01')` |
| `sort` | Comma-separated, `-` prefix for DESC | `-created,title` |
| `page` | Page number (1-based) | `1` |
| `perPage` | Items per page (default 30, max 200) | `20` |
| `expand` | Comma-separated relation fields to preload | `author,categories` |
| `fields` | Comma-separated field names to return | `id,title,created` |
| `skipTotal` | Skip total count query for performance | `1` |

#### Filter Compiler

Translate PocketBase filter syntax into Ecto dynamic queries:

```
(title ~ 'hello' && created > '2024-01-01') || (featured = true)
                                       │
                                       ▼
dynamic([r], (like(r.title, "%hello%") and r.created > ^~D[2024-01-01]) or r.featured == true)
```

### 2.4 PocketBase-Compatible API Response Format

```json
// GET /api/posts?page=1&perPage=2
{
  "page": 1,
  "perPage": 2,
  "totalItems": 42,
  "totalPages": 21,
  "items": [
    {
      "id": "abc123",
      "title": "Hello World",
      "body": "...",
      "created": "2024-01-15T10:30:00.000Z",
      "updated": "2024-01-15T10:30:00.000Z",
      "collectionId": "xyz",
      "collectionName": "posts"
    },
    { ... }
  ]
}

// GET /api/posts/abc123?expand=author
{
  "id": "abc123",
  "title": "Hello World",
  "body": "...",
  "collectionId": "xyz",
  "collectionName": "posts",
  "expand": {
    "author": {
      "id": "usr1",
      "name": "Alice",
      "collectionId": "yyy",
      "collectionName": "users"
    }
  }
}
```

### 2.5 Error Handling

Consistent error format matching PocketBase:

```json
// 400 — Validation error
{
  "code": 400,
  "message": "Something went wrong while processing your request.",
  "data": {
    "title": {
      "code": "validation_required",
      "message": "Missing required value."
    },
    "email": {
      "code": "validation_invalid_email",
      "message": "Invalid email format."
    }
  }
}

// 404
{
  "code": 404,
  "message": "The requested resource wasn't found.",
  "data": {}
}
```

### 2.6 Deliverables (Phase 2)

- [x] `DynamicController` with full CRUD (list/show/create/update/delete)
- [x] Filter compiler (PocketBase syntax → SQL WHERE clauses)
- [x] Sort, paginate, field selection
- [x] Relation expansion (`expand` parameter) via `DynamicView.expand_records`
- [x] PocketBase-compatible JSON response format
- [x] Error handling with canonical error format
- [x] Comprehensive ExUnit tests for filter/sort/paginate edge cases (68 filter + 21 controller tests)

---

## Phase 3 — Auth System

**Goal:** Auth collections, JWT auth, OAuth2 providers, and API key support.

**Duration:** ~1 week

### 3.1 Auth Collection Type

When a collection is created with `type: "auth"`, it automatically gets these system fields:

| Field | Type | System | Hidden | Description |
|---|---|---|---|---|
| `email` | email | no | no | Unique identifier |
| `password` | password | yes | yes | Never returned in API, bcrypt-hashed |
| `name` | text | no | no | Display name (optional) |
| `avatar` | text | no | no | Avatar URL (optional) |

### 3.2 JWT Implementation

```elixir
defmodule LazyPock.Auth.Token do
  @moduledoc """
  JWT creation and verification.
  Uses joken or a lightweight custom implementation.
  """

  @signing_alg "HS256"
  @access_token_ttl 3600 * 24 * 7  # 7 days (matching PocketBase default)
  @refresh_token_ttl 3600 * 24 * 30  # 30 days

  def generate_access_token(user, collection) do
    claims = %{
      "sub" => user.id,
      "type" => "access",
      "collection" => collection.name,
      "email" => user.email,
      "role" => user.role,
      "token_version" => user.token_version,
      "iat" => DateTime.utc_now() |> DateTime.to_unix(),
      "exp" => DateTime.utc_now() |> DateTime.add(@access_token_ttl) |> DateTime.to_unix()
    }
    Joken.generate_and_sign!(claims, signing_key())
  end

  def generate_refresh_token(user, collection) do
    # ...
  end

  def verify_and_validate(token) do
    with {:ok, claims} <- Joken.verify_and_validate(token, signing_key()) do
      # Check token_version matches current user's version
      #
      # (invalidates tokens after password change)
      {:ok, claims}
    end
  end
end
```

### 3.3 Auth Endpoints (PocketBase Compatible)

| Method | Path | Description |
|---|---|---|
| `POST` | `/:collection/auth-with-password` | Email + password → JWT token + user record |
| `POST` | `/:collection/auth-refresh` | Refresh JWT token (requires auth) |
| `GET` | `/:collection/auth-methods` | List available auth methods |
| `POST` | `/api/superusers/login` | Superuser login (legacy) |
| `POST` | `/api/superusers/setup` | First superuser setup |
| `POST` | `/api/auth/request-password-reset` | (planned) Send reset email |
| `POST` | `/api/auth/confirm-password-reset` | (planned) Reset password with token |
| `POST` | `/api/auth/request-verification` | (planned) Send verification email |
| `POST` | `/api/auth/confirm-verification` | (planned) Verify email |
| `GET` | `/api/auth/oauth2/{provider}` | (planned) OAuth2 flow |

### 3.4 Auth Plug Pipeline

```elixir
defmodule LazyPockWeb.AuthPlug do
  @moduledoc """
  Plug that authenticates requests and assigns current_user to conn.
  Supports: Bearer token (JWT), session cookie (admin UI), API key header.
  """

  def authenticate(conn, _opts) do
    case get_token(conn) do
      {:bearer, token} ->
        case LazyPock.Auth.Token.verify_and_validate(token) do
          {:ok, claims} ->
            user = LazyPock.Auth.lookup_user(claims["sub"], claims["collection"])
            assign(conn, :current_user, user)
          _ ->
            assign(conn, :current_user, nil)
        end

      {:api_key, key} ->
        # TODO: API key lookup
        assign(conn, :current_user, nil)

      :none ->
        assign(conn, :current_user, nil)
    end
  end

  defp get_token(conn) do
    # Check Authorization header, then X-Token header, then cookie
  end
end
```

### 3.5 OAuth2 Providers (via Assent)

```elixir
# Supported OAuth2 providers (configurable):
config :lazypock, :oauth2_providers, [
  google: [client_id: "...", client_secret: "..."],
  github: [client_id: "...", client_secret: "..."],
  # facebook, twitter, microsoft, apple, etc.
]
```

### 3.6 Deliverables (Phase 3)

- [x] Superuser auth table + schema (`_superusers`)
- [x] JWT token generation + verification
- [x] Superuser login/setup/me/check endpoints
- [x] Superuser auto-creation from env vars (`LAZYPOCK_SUPERUSER_*`)
- [x] Auth plug middleware (`Lazypock.Auth.Plug`)
- [x] Auth collection type with system fields (email, password, role)
- [x] Auth collection JWT tokens (`generate_user_token/2`, `verify_user_token/1`)
- [x] PocketBase-compatible auth endpoints (`/:collection/auth-with-password`, `/:collection/auth-refresh`, `/:collection/auth-methods`)
- [x] AuthController (`auth_with_password`, `auth_refresh`, `auth_methods`)
- [x] Dynamic email/password field resolution from collection schema
- [ ] OAuth2 provider support
- [x] Rate limiting on login attempts

---

## Phase 4 — Rule Engine

**Goal:** PocketBase-style access control rules that compile to Ecto queries + Elixir guards.

**Duration:** ~1 week

### 4.1 Rule Storage

Rules are stored per collection in the `rules` JSONB column:

```json
{
  "listRule":   "@request.auth.id != ''",
  "viewRule":   "id = @request.auth.id || @request.auth.role = 'admin'",
  "createRule": "@request.auth.id != ''",
  "updateRule": "owner_id = @request.auth.id || @request.auth.role = 'admin'",
  "deleteRule": "owner_id = @request.auth.id",
  "manageRule": null
}
```

### 4.2 Rule Syntax (PocketBase Compatible)

| Expression | Example |
|---|---|
| Literals | `true`, `false`, `null`, `1`, `"hello"`, `'hello'` |
| Field access | `title`, `status`, `owner_id` |
| Auth variables | `@request.auth.id`, `@request.auth.email`, `@request.auth.role` |
| Comparisons | `=`, `!=`, `>`, `>=`, `<`, `<=`, `~` (like / contains) |
| Logical | `&&`, `\|\|`, `!` |
| Parentheses | `(a && b) \|\| c` |

### 4.3 Compiler Architecture

```
Rule string: "owner_id = @request.auth.id || @request.auth.role = 'admin'"
        │
        ▼
┌──────────────────┐
│     Tokenizer    │  →  [{:field, "owner_id"}, {:eq}, {:auth, "id"}, {:or}, ...]
└────────┬─────────┘
         ▼
┌──────────────────┐
│      Parser      │  →  {:or, {:eq, {:field, "owner_id"}, {:auth, "id"}},
└────────┬─────────┘          {:eq, {:field, "role"}, {:literal, "admin"}}}
         ▼
┌──────────────────┐
│     Compiler     │  →  {:or, {:ownership, :owner_id},
└────────┬─────────┘          {:field_eq, :role, "admin"}}
         ▼
┌──────────────────┐
│ Ecto Query Gen   │  →  where: r.owner_id == ^user.id or r.role == "admin"
└──────────────────┘
```

### 4.4 Applying Rules

**List/View** — Modify Ecto query WHERE clause:

```elixir
def apply_list_rule(query, collection, user) do
  rule = collection.rules["listRule"] || ""
  if rule == "", do: query, else: compile_and_apply(query, rule, user)
end
```

**Create** — Validate attributes BEFORE insert:

```elixir
def authorize_create(collection, attrs, user) do
  rule = collection.rules["createRule"] || ""
  if rule == "", do: :ok, else: evaluate_rule(rule, attrs, user)
end
```

**Update** — Load existing record, evaluate rule against it:

```elixir
def authorize_update(collection, record, new_attrs, user) do
  rule = collection.rules["updateRule"] || ""
  if rule == "", do: :ok, else: evaluate_rule(rule, record, user)
end
```

**Delete** — Same as update but with deleteRule:

```elixir
def authorize_delete(collection, record, user) do
  rule = collection.rules["deleteRule"] || ""
  if rule == "", do: :ok, else: evaluate_rule(rule, record, user)
end
```

### 4.5 Default Rules

```elixir
defmodule LazyPock.Rules.Defaults do
  def for_auth_collection do
    %{
      "listRule"   => "@request.auth.id != ''",
      "viewRule"   => "id = @request.auth.id",
      "createRule" => "",  # Anyone can register
      "updateRule" => "id = @request.auth.id",
      "deleteRule" => "id = @request.auth.id",
      "manageRule" => "@request.auth.role = 'admin'"
    }
  end

  def for_base_collection do
    %{
      "listRule"   => "",
      "viewRule"   => "",
      "createRule" => "@request.auth.id != ''",
      "updateRule" => "",
      "deleteRule" => "",
      "manageRule" => "@request.auth.role = 'admin'"
    }
  end
end
```

### 4.6 Deliverables (Phase 4)

- [x] Rules stored as JSONB in `_collections.rules`
- [x] `Enforcer` module — three-state logic (nil/""/filter)
- [x] `authorize_list/2` — returns SQL WHERE clause for query modification
- [x] `authorize_create/3` — in-memory rule evaluation for attrs
- [x] `authorize_update/4` — in-memory rule evaluation for existing record
- [x] `authorize_delete/3` — in-memory rule evaluation for record
- [x] `authorize_manage/2` — manageRule for delegated collection management
- [x] `@request.auth.*` variable resolution with SQL evaluation
- [x] Superuser bypass (superusers always pass all rules)
- [x] Rule validation in Studio (syntax checking) — `ruleValidator.ts` + `RuleField.svelte`
- [x] Sensible defaults per collection type (auth vs base rules in DDL)
- [x] ExUnit tests for rule scenarios

---

## Phase 5 — Realtime (Phoenix Channels)

**Goal:** Real-time subscriptions for collection CRUD events. PocketBase-compatible client experience.

**Duration:** ~1 week

### 5.1 Channel Topics

| Topic | Fires On |
|---|---|
| `collection:*` | Any create/update/delete in the collection |
| `collection:{id}` | Any change to a specific record |
| `collection:{id}/*` | Any change to a specific record (alias) |
| `collection:{id}/{relation}` | Changes to related records (stretch goal) |

### 5.2 Server-Side

```elixir
defmodule LazyPockWeb.CollectionChannel do
  use Phoenix.Channel

  @impl true
  def join("collection:" <> topic, _payload, socket) do
    # topic can be: "posts", "posts:abc123", "posts:*"
    {:ok, assign(socket, :subscription, topic)}
  end

  # Clients send subscribe/unsubscribe messages
  @impl true
  def handle_in("subscribe", %{"recordId" => record_id}, socket) do
    # Add subscription for a specific record
    {:noreply, socket}
  end
end
```

### 5.3 Broadcasting (Wired into Data Layer)

```elixir
defmodule LazyPock.Realtime.Broadcaster do
  @moduledoc """
  Broadcasts CRUD events to all subscribed clients.
  Called from controllers after DB operations.
  """

  alias LazyPockWeb.Endpoint

  def broadcast_create(collection_name, record) do
    payload = %{
      action: "create",
      record: serialize(record)
    }

    # Broadcast to wildcard subscribers
    Endpoint.broadcast!("collection:#{collection_name}", "record_change", payload)
    Endpoint.broadcast!("collection:#{collection_name}:*", "record_change", payload)
  end

  def broadcast_update(collection_name, record) do
    payload = %{
      action: "update",
      record: serialize(record)
    }

    Endpoint.broadcast!("collection:#{collection_name}", "record_change", payload)
    Endpoint.broadcast!("collection:#{collection_name}:*", "record_change", payload)
    Endpoint.broadcast!("collection:#{collection_name}:#{record.id}", "record_change", payload)
  end

  def broadcast_delete(collection_name, record_id) do
    payload = %{
      action: "delete",
      record: %{id: record_id}
    }

    Endpoint.broadcast!("collection:#{collection_name}", "record_change", payload)
    Endpoint.broadcast!("collection:#{collection_name}:*", "record_change", payload)
    Endpoint.broadcast!("collection:#{collection_name}:#{record_id}", "record_change", payload)
  end
end
```

### 5.4 Client SDK Design (JavaScript)

```javascript
// PocketBase-compatible client experience
import LazyPock from "lazypock";

const client = new LazyPock("https://myapp.fly.dev");

// Subscribe to all changes in "posts" collection
client.collection("posts").subscribe("*", (e) => {
  console.log(e.action); // "create" | "update" | "delete"
  console.log(e.record); // Full record or {id} on delete
});

// Subscribe to a single record
client.collection("posts").subscribe("abc123", (e) => {
  console.log(`Post abc123 changed: ${e.action}`);
});

// Unsubscribe
client.collection("posts").unsubscribe("abc123");
client.collection("posts").unsubscribe(); // all subscriptions for this collection
```

### 5.5 Deliverables (Phase 5)

- [x] Phoenix Channel with collection-aware topics (`collection:name`, `collection:name:{id}`)
- [x] `Realtime.Broadcaster` wired into DynamicController
- [x] Rule enforcement on channel join (checks listRule)
- [x] Admin channel for collection CRUD events
- [x] Client-side JS SDK subscriptions (basic `RealtimeService` exists)
- [x] Reconnection handling (heartbeat, `onclose`→`scheduleReconnect`, `resubscribeAll`)
- [x] Auth integration (channels authenticate via JWT)

---

## Phase 6 — File Storage

**Goal:** File upload, retrieval, thumbnail generation, and pluggable storage backends.

**Duration:** ~1 week

### 6.1 File Handling Design

```elixir
# _files table (system table)
defmodule LazyPock.Files.File do
  use Ecto.Schema
  schema "_files" do
    field :filename, :string         # original filename
    field :extension, :string        # ".jpg"
    field :mime_type, :string        # "image/jpeg"
    field :size, :integer            # bytes
    field :storage_path, :string     # relative to storage root
    field :storage_backend, :string  # "local" | "s3"
    field :meta, :map               # {width, height, thumbnails_generated, ...}
    field :collection_name, :string  # which collection this file belongs to
    field :record_id, :binary_id     # which record
    field :field_name, :string       # which field on the record
    timestamps()
  end
end
```

### 6.2 Upload Endpoint

```elixir
# POST /api/files
# Body: multipart/form-data with "file" field
def upload(conn, %{"file" => upload}) do
  with {:ok, file_record} <- LazyPock.Files.Store.upload(upload, opts) do
    conn |> put_status(201) |> render(:file, file: file_record)
  end
end

# GET /api/files/:id
# Returns the raw file with correct Content-Type
def show(conn, %{"id" => id}) do
  file = LazyPock.Files.get!(id)
  path = LazyPock.Files.Store.local_path(file)
  conn
  |> put_resp_header("content-type", file.mime_type)
  |> send_file(200, path)
end

# GET /api/files/:id?thumb=100x100
# Returns a thumbnail variant
def show(conn, %{"id" => id, "thumb" => dimensions}) do
  file = LazyPock.Files.get!(id)
  thumb_path = LazyPock.Files.Thumbnail.get_or_generate(file, dimensions)
  conn |> send_file(200, thumb_path)
end
```

### 6.3 Storage Backends (Pluggable Adapter Pattern)

```elixir
defmodule LazyPock.Files.Adapter do
  @callback put(binary(), String.t(), keyword()) :: {:ok, map()} | {:error, term()}
  @callback delete(String.t()) :: :ok | {:error, term()}
  @callback get_url(String.t()) :: String.t()
  @callback exists?(String.t()) :: boolean()
end

# Implementations:
# - LazyPock.Files.Adapters.Local  (default, files on disk)
# - LazyPock.Files.Adapters.S3     (AWS S3 compatible)
# - LazyPock.Files.Adapters.GCS    (Google Cloud Storage)
```

### 6.4 Thumbnail Generation

```elixir
# Using Vix (libvips bindings) for image processing
defmodule LazyPock.Files.Thumbnail do
  def generate(file, width, height) do
    thumb_path = thumb_path(file, width, height)

    {:ok, _} = Vix.Vips.Operation.thumbnail(file.storage_path, width,
      height: height,
      crop: :centre
    )
    |> Vix.Vips.Image.write_to_file(thumb_path)

    thumb_path
  end
end
```

### 6.5 Deliverables (Phase 6)

- [x] `_files` table auto-created on boot
- [x] File upload endpoint (`POST /api/files`)
- [x] File serve endpoint (`GET /api/files/:id`)
- [x] File delete endpoint (`DELETE /api/files/:id`)
- [x] Local storage adapter (`Lazypock.Files.Adapters.Local`)
- [x] S3 adapter (`Lazypock.Files.Adapters.S3`)
- [ ] Thumbnail generation (Vix/libvips)

---

## Phase 7 — Hook System

**Goal:** Three-layer hook system — declarative (JSON), file-based (Elixir), and runtime (sandboxed eval).

**Duration:** ~1.5 weeks

### 7.1 Layer 1: Declarative Hooks (Admin UI, Zero Code)

```json
{
  "hooks": {
    "onCreate": [
      {
        "type": "set_field",
        "field": "slug",
        "value": "{{slugify record.title}}"
      },
      {
        "type": "webhook",
        "url": "https://external-api.com/new-post",
        "method": "POST",
        "headers": {"Authorization": "Bearer {{env.WEBHOOK_SECRET}}"},
        "body": {"id": "{{record.id}}", "title": "{{record.title}}"}
      }
    ],
    "onUpdate": [
      {
        "type": "set_field", "field": "updated_at", "value": "{{now}}"
      }
    ],
    "onDelete": [
      {
        "type": "send_email",
        "to": "admin@site.com",
        "template": "record-deleted",
        "data": {"collection": "{{collection.name}}", "id": "{{record.id}}"}
      }
    ],
    "afterCreate": [],
    "afterUpdate": [],
    "afterDelete": [
      {
        "type": "log",
        "level": "info",
        "message": "Record {{record.id}} deleted from {{collection.name}}"
      }
    ]
  }
}
```

#### Built-in Declarative Actions

| Action | Description |
|---|---|
| `set_field` | Set a field value dynamically (supports `{{template}}` variables) |
| `webhook` | Call an external URL |
| `send_email` | Send email via configured provider |
| `log` | Write to application logs |
| `increment` | Increment a numeric field |
| `push` | Push a value to an array field |
| `validate` | Custom validation with error message |
| `block` | Block the operation with a reason |

### 7.2 Layer 2: File-Based Elixir Hooks (PocketBase Event Hooks API)

The hook system mirrors the [PocketBase JS event hooks](https://pocketbase.io/docs/js-event-hooks/)
API — same hook names, same `function(e)` + `e.next()` event-chain semantics,
adapted to Elixir.

Users create `.ex` files in `priv/hooks/` (auto-discovered at boot):

```elixir
# priv/hooks/posts_hooks.ex
defmodule PostsHooks do
  use Lazypock.Hooks.Hook, collection: "posts"
  alias Lazypock.Hooks.Event

  # PocketBase: onRecordCreate((e) => { e.record.slug = ...; e.next() })
  def on_record_create(%Event{} = e) do
    slug =
      e.record["title"]
      |> to_string()
      |> String.downcase()
      |> String.replace(~r/[^a-z0-9]+/, "-")
      |> String.trim("-")

    e = Event.put(e, :record, Map.put(e.record, "slug", slug))
    Event.next(e)
  end

  # PocketBase: onRecordValidate — reject if empty title
  def on_record_validate(%Event{} = e) do
    if e.record["title"] == "" do
      {:error, "title is required"}
    else
      Event.next(e)
    end
  end

  # PocketBase: onRecordAfterCreateSuccess
  def on_record_after_create_success(%Event{} = e) do
    Logger.info("post created: #{e.record["id"]}")
    Event.next(e)
  end
end
```

#### Event-chain semantics (PocketBase parity)

Every handler has the same `function(e)` signature. Calling `Event.next(e)`
(or returning `{:ok, e}`) proceeds with the chain. Returning `{:error, reason}`
(or raising) — or **not calling `e.next()`** — stops the chain, exactly like
PocketBase.

Handlers that need to run code **after** the DB action pass a callback to
`Event.next/2` (PocketBase's "operations AFTER `e.next()`"):

```elixir
def on_record_create(%Event{} = e) do
  e = Event.put(e, :record, Map.put(e.record, "slug", slugify(e.record["title"])))
  Event.next(e, fn e ->
    # runs after the record is persisted
    Logger.info("created #{e.record["id"]}")
    :ok
  end)
end
```

#### Available hooks (full PocketBase surface)

- **App**: `on_bootstrap`, `on_settings_reload`, `on_backup_create`,
  `on_backup_restore`, `on_terminate`, `on_before_serve` (custom API routes)
- **Record model**: `on_record_enrich`, `on_record_validate` + the
  create/update/delete × execute/after-success/after-error matrix
- **Collection model**: same 12-hook matrix
- **Base model**: `on_model_validate` + create/update/delete matrix
- **Request**: records CRUD, auth (auth, auth-refresh, auth-with-password,
  OAuth2, OTP, password-reset, verification, email-change), batch, file,
  collections, settings
- **Mailer**: `on_mailer_send` + `on_mailer_record_*Send` variants
- **Realtime**: `on_realtime_connect_request`, `on_realtime_subscribe_request`,
  `on_realtime_message_send`

Hook modules are scoped to a collection with `use Lazypock.Hooks.Hook,
collection: "posts"` (PocketBase's trailing collection args).

#### Custom API routes (PocketBase `onBeforeServe` / `routerAdd`)

```elixir
def on_before_serve(%Event{} = e) do
  routes = Lazypock.Hooks.Router.add(e, "GET", "/hello/{name}", fn ctx ->
    Lazypock.Hooks.Router.json(ctx, 200, %{"message" => "Hello #{ctx.params["name"]}"})
  end)
  e = Event.put(e, :routes, routes)
  Event.next(e)
end
```

#### Lifecycle Behaviour (deprecated, still supported)

```elixir
defmodule LazyPock.Hooks.Lifecycle do
  @callback on_create(map(), LazyPock.Hooks.Context.t()) :: {:ok, map()} | {:error, term()} | :skip
  @callback after_create(map(), LazyPock.Hooks.Context.t()) :: :ok
  @callback on_update(map(), map(), LazyPock.Hooks.Context.t()) :: {:ok, map()} | {:error, term()}
  @callback after_update(map(), map(), LazyPock.Hooks.Context.t()) :: :ok
  @callback on_delete(map(), LazyPock.Hooks.Context.t()) :: :ok | {:error, term()}
  @callback after_delete(map(), LazyPock.Hooks.Context.t()) :: :ok
  @callback validate(map(), LazyPock.Hooks.Context.t()) :: :ok | {:error, keyword()}
  @callback can_access(map(), atom(), LazyPock.Hooks.Context.t()) :: boolean()

  @optional_callbacks [
    on_create: 2, after_create: 2, on_update: 3, after_update: 3,
    on_delete: 2, after_delete: 2, validate: 2, can_access: 3
  ]

  defmacro __using__(opts) do
    collection = Keyword.fetch!(opts, :collection)
    quote do
      @behaviour LazyPock.Hooks.Lifecycle
      @collection unquote(collection)

      def on_create(record, _ctx), do: {:ok, record}
      def after_create(_record, _ctx), do: :ok
      def on_update(_old, new_attrs, _ctx), do: {:ok, new_attrs}
      def after_update(_old, _new, _ctx), do: :ok
      def on_delete(_record, _ctx), do: :ok
      def after_delete(_record, _ctx), do: :ok
      def validate(_record, _ctx), do: :ok
      def can_access(_record, _action, _ctx), do: true

      defoverridable on_create: 2, after_create: 2, on_update: 3,
                     after_update: 3, on_delete: 2, after_delete: 2,
                     validate: 2, can_access: 3
    end
  end
end
```

### 7.3 Layer 3: Runtime Eval Hooks (Admin UI, Advanced)

⚠️ **Security note**: Uses `Code.eval_string` with sandboxing. Only for admin users.

```elixir
defmodule LazyPock.Hooks.Runtime do
  def execute(collection, :on_create, record, user) do
    code = get_runtime_hook(collection, "onCreate")

    if code do
      # Run in a supervised Task with 100ms timeout
      task = Task.Supervisor.async_nolink(LazyPock.TaskSupervisor, fn ->
        safe_eval(code, %{record: record, user: user, action: :create})
      end)

      case Task.yield(task, 100) || Task.shutdown(task) do
        {:ok, {:ok, modified}} -> {:ok, modified}
        {:ok, {:error, reason}} -> {:error, reason}
        nil -> {:error, :hook_timeout}
      end
    else
      {:ok, record}
    end
  end

  # Restricted eval environment
  defp safe_eval(code, bindings) do
    imports = """
    alias String, as: S
    alias Enum, as: E
    alias Map, as: M
    alias List, as: L
    alias DateTime, as: DT
    alias Jason, as: J
    """

    full_code = "#{imports}\nfn %{record: record, user: user} -> #{code} end"
    {fun, _} = Code.eval_string(full_code, [], __ENV__)
    fun.(bindings)
  end
end
```

### 7.4 Hook Dispatcher (Pipeline Orchestrator)

```elixir
defmodule LazyPock.Hooks.Dispatcher do
  @moduledoc """
  Orchestrates hook execution across all three layers.
  Pre-hooks run sequentially (blocking), after-hooks run async (fire-and-forget).
  """

  def dispatch_create(collection, attrs, context) do
    pipeline = [
      &Layer1.declarative_create/3,
      &Layer3.runtime_create/3,
      &Layer2.file_based_create/3,
    ]

    # Sequential: each hook can modify or reject
    run_pipeline(pipeline, collection, attrs, context)
  end

  def dispatch_after_create(collection, record, context) do
    # Fire-and-forget: errors logged, never block the response
    Task.Supervisor.start_child(LazyPock.TaskSupervisor, fn ->
      Layer1.declarative_after_create(collection, record, context)
      Layer2.file_based_after_create(collection, record, context)
      Layer3.runtime_after_create(collection, record, context)
    end)
  end

  defp run_pipeline(steps, collection, data, context) do
    Enum.reduce_while(steps, {:ok, data}, fn step, {:ok, current} ->
      case step.(collection, current, context) do
        {:ok, modified} -> {:cont, {:ok, modified}}
        {:error, reason} -> {:halt, {:error, reason}}
        :skip -> {:halt, {:skip, reason}}
      end
    end)
  end
end
```

### 7.5 Hook Registry (Auto-Discovery)

Scans `priv/hooks/` at boot, maps collection names to hook modules, supports hot-reload.

### 7.6 Deliverables (Phase 7)

- [x] Layer 2: File-based Elixir hooks — PocketBase-parity event API (`use Lazypock.Hooks.Hook`, `e.next()` chain, `Router.add` custom API routes) + auto-discovery (`Registry.discover!()`)
- [x] Hook dispatcher pipeline wired into DynamicController (+ Auth/Collection/Settings/File controllers, CollectionChannel, Mailer)
- [x] ExUnit: hook pipeline tests (Event chain semantics, Registry, Router, custom-routes end-to-end)
- [ ] Layer 1: Declarative hook engine with built-in actions (set_field, webhook, etc.) — skeleton exists (`run_declarative_hooks/4` returns `{:ok, data}` unchanged)
- [ ] Layer 3: Runtime eval hooks (Studio admin UI)
- [ ] Template variable system (`{{record.title}}`, `{{now}}`, `{{env.VAR}}`)
- [ ] Hook execution logging/tracing

---

## Phase 8 — Studio Admin SPA (SvelteKit)

**Goal:** Feature-complete admin dashboard for managing collections, records, rules, hooks, files, and auth.

**Duration:** ~2.5 weeks

### 8.1 Layout & Navigation

```
┌────────────────────────────────────────────────┐
│  🦥 LazyPock Admin              [🔔] [👤]  ⚙   │
├──────────┬─────────────────────────────────────┤
│          │                                     │
│ 🏠 Home  │                                     │
│          │                                     │
│ 📦 Coll. │          Main Content Area           │
│  ├ posts │                                     │
│  ├ users │                                     │
│  └ tags  │                                     │
│          │                                     │
│ 📁 Files │                                     │
│          │                                     │
│ ⚙ Settings│                                     │
│  ├ Auth  │                                     │
│  ├ Hooks │                                     │
│  └ Logs  │                                     │
│          │                                     │
└──────────┴─────────────────────────────────────┘
```

### 8.2 Pages / Components

#### Home Dashboard

- Total collections, records, files, users
- Recent activity feed
- System health (DB status, storage usage, uptime)
- Quick actions (create collection, upload file)

#### Collection Manager

- List all collections with type (base/auth), record count, created date
- Create new collection flow (name, type, fields...)
- Edit / Delete collection
- Schema visualizer (field names, types, constraints)

#### Field Editor (Collection Builder)

- Add / Remove / Reorder fields
- Configure each field:
  - Name, type (dropdown), required, unique, default
  - Type-specific options (max_length, values for select, etc.)
  - Index toggle
- Real-time validation (no duplicate names, reserved words check)
- Preview of generated SQL

#### Record Browser

- Table view with sortable columns
- Filter bar (PocketBase filter syntax with autocomplete)
- Pagination controls
- Bulk actions (delete, export)
- Create / Edit / Delete individual records
- Dynamic form generated from field definitions
- File upload for file fields
- Relation picker for relation fields

#### Rule Editor

- Per-collection rule editor with syntax highlighting
- Rule builder UI (visual, no code needed for simple rules)
- Rule tester: "Test this rule as user X, record Y"
- Pre-built rule templates

#### Auth Manager

- View auth collections and their users
- Manually verify/unverify users
- Password reset workflow
- OAuth2 provider configuration
- Role management

#### Hook Manager

- Declarative hook editor (UI-based, dropdowns for actions)
- File-based hook viewer (read-only, shows status)
- Runtime hook code editor (monaco/codemirror with Elixir highlighting)
- Hook execution log viewer

#### File Browser

- Thumbnail grid view for images
- List view with metadata (size, type, uploaded date)
- Upload (drag & drop)
- Delete
- Filter by collection/record

#### Logs / Audit Trail

- Hook execution logs
- Auth events (login, logout, failed attempts)
- Schema changes (collection created, field added, etc.)
- Error logs
- Filterable by collection, event type, date range

#### Settings

- Storage backend config
- Email provider config
- Rate limiting config
- Environment config view

### 8.3 Studio Component Tree

```
studio/src/routes/
├── +layout.svelte           # Auth guard, realtime init
├── login/+page.svelte       # Superuser login/setup
└── (app)/
    ├── +layout.svelte        # App shell with sidebar
    └── collections/+page.svelte  # Main: sidebar + DataTable + SidePane
        ├── DataTable.svelte  # Generic tabular view
        ├── SidePane.svelte   # Slide-out panel for forms
        ├── FieldSettings.svelte  # Per-field config
        ├── NewFieldButton.svelte # Add field button
        ├── RecordForm.svelte # Dynamic form from schema
        ├── RuleField.svelte  # Lock/unlock rule input
        ├── IndexesModal.svelte    # Unique/index manager
        ├── Dropdown.svelte   # Menu dropdown
        ├── Button.svelte     # Styled button
        ├── Input.svelte      # Styled input
        ├── Modal.svelte      # Generic modal
        └── OptionRow.svelte  # Select options row
```

### 8.4 Deliverables (Phase 8)

- [x] **Studio SvelteKit SPA** serves at `/_/*` (replaces LiveView dashboard)
- [x] Auth guard + login page with token persistence
- [x] Collection sidebar with user/system grouping
- [x] Collection CRUD (create/edit/delete) in slide-out pane
- [x] Field editor (add, remove, reorder fields)
- [x] Record browser with dynamic schema DataTable
- [x] Record CRUD (create/edit/delete) with form validation
- [x] API Rules tab with per-rule lock/unlock (including manageRule)
- [x] Unique constraint/indexes management modal
- [ ] Home dashboard with stats/health
- [ ] Auth manager (user management)
- [ ] Hook manager
- [ ] File browser
- [x] Logs / audit trail viewer (`/logs` with stats + chart)
- [x] Settings page (application, backups, cron, export, files, import, mail, SQL console)

---

## Phase 9 — Client SDKs

**Goal:** Official client libraries for JavaScript/TypeScript, with Dart/Flutter and Python as stretch goals.

**Duration:** ~1 week (JS), ongoing for others

### 9.1 JavaScript/TypeScript SDK (`lazypock`)

PocketBase-compatible API:

```typescript
import LazyPock from "lazypock";

const client = new LazyPock("https://myapp.fly.dev");

// ── Auth ────────────────────────────────────────
await client.auth.login("user@example.com", "password");
await client.auth.register({ email, password, passwordConfirm });
await client.auth.refresh();
client.auth.isAuthenticated; // boolean
client.auth.user; // current user record

// ── CRUD ────────────────────────────────────────
const posts = await client.collection("posts").getList(1, 20, {
  filter: "created > '2024-01-01'",
  sort: "-created",
  expand: "author",
});
const post = await client.collection("posts").getOne("abc123");
const newPost = await client.collection("posts").create({ title: "Hello" });
const updated = await client.collection("posts").update("abc123", { title: "Updated" });
await client.collection("posts").delete("abc123");

// ── Realtime ────────────────────────────────────
const unsub = await client.collection("posts").subscribe("*", (e) => {
  console.log(e.action, e.record);
});
await client.collection("posts").subscribe("abc123", (e) => { /* single record */ });
unsub(); // unsubscribe

// ── Files ───────────────────────────────────────
const files = await client.files.getList(1, 20);
const fileUrl = client.files.getUrl(fileId, { thumb: "100x100" });
const uploaded = await client.files.upload(formData);
await client.files.delete(fileId);
```

### 9.2 Dart/Flutter SDK (`lazypock-dart`) — Stretch

### 9.3 Python SDK (`lazypock-py`) — Stretch

### 9.4 Deliverables (Phase 9)

- [x] `lazypock` npm package (publishable)
- [x] Superuser auth flow (login, me, logout)
- [x] Full CRUD for records (list/getOne/create/update/delete)
- [x] Collection management SDK methods
- [x] `AuthStore` with localStorage persistence
- [x] `RealtimeService` with Phoenix Channel WebSocket protocol
- [x] TypeScript types (`ApiRecord`, `ListResult`, `AuthModel`)
- [x] File upload/download — `FilesService` with `upload()`/`getUrl()`/`delete()`
- [x] Auto-refresh on token expiry — transparent refresh via `HttpClient.request()` interceptor
- [x] README + API docs + examples (README + JSDoc in the `lazypock-ts` repo)

---

## Phase 10 — Polish & Release

**Goal:** Production hardening, documentation, CLI, release pipeline.

**Duration:** ~2 weeks

### 10.1 CLI (`mix lazypock`)

```bash
# Scaffold a new LazyPock project
mix lazypock.new my-backend

# Start the server
mix lazypock.server
# or: MIX_ENV=prod mix phx.server

# Create a collection from CLI
mix lazypock.collections.create posts title:text body:text published:bool

# Export / import collections (for git tracking)
mix lazypock.schema.export  # → priv/schema.json
mix lazypock.schema.import priv/schema.json

# Database management
mix lazypock.db.backup   # pg_dump
mix lazypock.db.restore  # pg_restore

# Generate a static Ecto schema for a collection (eject from dynamic)
mix lazypock.eject posts  # → lib/my_app/schemas/posts.ex
```

### 10.2 Production Checklist

| Area | Action |
|---|---|
| **Security** | Audit all `Code.eval_string` paths, ensure sandboxing |
| **Security** | Rate limiting on auth endpoints |
| **Security** | CORS configuration |
| **Security** | Input sanitization for DDL (SQL injection prevention) |
| **Performance** | Query optimization (ensure proper indexes) |
| **Performance** | ETS cache warming strategy for cold starts |
| **Performance** | Connection pool sizing guide |
| **Reliability** | DDL transaction safety (comprehensive tests) |
| **Reliability** | Graceful error recovery in hook pipeline |
| **Reliability** | PubSub failover for multi-node |
| **Observability** | Telemetry integration (metrics, traces) |
| **Observability** | Structured logging (JSON) |
| **Deployment** | `mix release` → single tarball |
| **Deployment** | Docker image + docker-compose |
| **Deployment** | Fly.io / Gigalixir / Render deployment guides |
| **Deployment** | Environment variable reference |

### 10.3 Documentation

- [ ] Getting started guide (< 5 minutes to first API)
- [ ] Collection & field reference
- [ ] Auth guide (login, OAuth2, API keys)
- [ ] Rules guide (syntax, examples, common patterns)
- [ ] Hooks guide (all three layers, with recipes)
- [ ] File storage guide
- [ ] Realtime guide
- [ ] Admin UI guide
- [ ] Deployment guides (Fly.io, Docker, bare metal)
- [ ] Migration guide from PocketBase
- [ ] API reference (generated)
- [ ] Example projects (blog, todo, chat)

### 10.4 Testing Strategy

| Type | Scope |
|---|---|
| **Unit tests** | DDL engine, type mapper, rule compiler, hook dispatcher |
| **Integration tests** | Full CRUD lifecycle per collection type |
| **Channel tests** | Realtime subscription/notification flow |
| **Studio UI tests** | Admin SPA forms and flows |
| **Property-based tests** | Filter parsing, rule compilation |
| **Load tests** | Concurrent DDL + CRUD on 100 collections |

### 10.5 Release Checklist

- [ ] Hex.pm package published (`lazypock`)
- [ ] npm package published (`lazypock`)
- [ ] GitHub repo with CI/CD (GitHub Actions) for `core/`, `sdk-js/` subprojects
- [ ] CHANGELOG.md
- [ ] CONTRIBUTING.md
- [ ] LICENSE (MIT)
- [ ] Logo + brand assets
- [ ] Landing page / website
- [ ] Demo video (2 minutes)
- [ ] Hacker News launch post draft
- [ ] Elixir Forum announcement

### 10.6 Deliverables (Phase 10)

- [ ] CLI tool with `new`, `server`, `collections.*`, `schema.*` subcommands
- [x] `mix release` single tarball build (Burrito)
- [ ] Docker image + docker-compose
- [ ] Comprehensive documentation
- [ ] Full test suite (unit + integration + channel)
- [ ] CI/CD pipeline
- [ ] Hex.pm + npm publication
- [ ] Deployment guides
- [ ] Brand assets + landing page

---

## Appendix: System Tables Reference

| Table | Purpose |
|---|---|
| `_collections` | Collection metadata (name, type, schema snapshot, rules, hooks, options) |
| `_fields` | Field definitions per collection (name, type, constraints, options) |
| `_files` | File metadata (filename, type, size, storage path, ownership) |
| `_migrations` | Ecto's own migration tracking (kept separate) |
| `_hooks_executions` | Audit log of all hook executions (future phase) |
| `_auth_tokens` | Active tokens tracking for revocation (future phase) |
| `_settings` | Application-wide settings (future phase) |

---

## Appendix: Technology Stack

| Layer | Technology | Why |
|---|---|---|
| **Language** | Elixir 1.17+ | Functional, concurrent, fault-tolerant |
| **Framework** | Phoenix 1.7+ | Battle-tested web framework |
| **Database** | PostgreSQL 15+ | Transactional DDL, JSONB, mature |
| **DB Library** | Ecto 3.11+ | Composable queries, dynamic sources |
| **Auth** | Joken + bcrypt_elixir | JWT + hashing |
| **OAuth2** | Assent | Multi-provider |
| **Admin UI** | SvelteKit (Studio SPA) | Admin dashboard served at `/_/*` |
| **File Processing** | Vix (libvips) | Fast image processing |
| **File Upload** | Custom | To match PocketBase semantics |
| **Realtime** | Phoenix PubSub + Channels | Built-in |
| **Release** | `mix release` | Single tarball deployment |
| **JS SDK** | TypeScript + Phoenix Channels client | Types + realtime |
| **CSS** | Tailwind CSS (optional) | Admin UI styling |
