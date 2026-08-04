# Lazypock — TypeScript SDK

TypeScript client library for [Lazypock](https://github.com/gnuzd/lazypock), an open-source PocketBase-compatible backend.

## Installation

```bash
npm install lazypock
```

## Quick Start

```typescript
import { LazypockClient } from 'lazypock';

const client = new LazypockClient({ baseUrl: 'http://localhost:4000/api' });

// Superuser login
await client.login('admin@example.com', 'password');

// Or auth collection login
await client.login('user@example.com', 'password', 'users');
// Or using the explicit method:
await client.authWithPassword('users', 'user@example.com', 'password');

// List records
const posts = await client.collection('posts').getList(1, 30);
// or fetch all pages:
const all = await client.collection('posts').getFullList();

// Create a record
const newPost = await client.collection('posts').create({ title: 'Hello', published: true });

// File upload
const file = await client.files.upload(fileInput.files[0]);

// Real-time subscriptions (PocketBase-style: callback-first)
client.collection('posts').subscribe((e) => console.log(e.action, e.record));
```

## Type Safety

The SDK offers three levels of type safety — pick what fits your project.

### 1. Fully typed via codegen (recommended)

Connect to your API once and generate a typed client — every collection becomes
an interface with the exact field types from your schema (selects become string
unions, relations become record IDs, etc.).

```bash
# In your app, after installing lazypock:
npx lazypock-gen \
  --url http://localhost:4000/api \
  --email admin@example.com \
  --password your-password
# writes ./lazypock.types.ts
```

> `lazypock-gen` remains as a deprecated alias for backwards compatibility —
> the canonical command is now simply `lazypock`:
>
> ```bash
> npx lazypock --url http://localhost:4000/api --email admin@example.com --password your-password
> ```
>
> **Use an API key instead of a password** (recommended). Generate one from the
> Studio **Settings → API Keys** dashboard, then:
>
> ```bash
> npx lazypock --url http://localhost:4000/api --apikey lazypock_xxxxxxxx
> # or via env: LAZYPOCK_URL=... LAZYPOCK_API_KEY=... npx lazypock
> ```
>
> API keys are stored as a SHA-256 hash (raw value shown once at generation) and
> are scoped to collection listing — ideal for codegen (they can `GET /collections`
> without a login round-trip, and cannot read or mutate your records).

Then in your app:

```typescript
import { createClient } from './lazypock.types';

const client = createClient({ baseUrl: 'http://localhost:4000/api' });
await client.login('admin@example.com', 'password');

// Collection access is fully type-checked:
const post = await client.collection('posts').getOne('abc123');
// post.title — string, post.published — boolean, …

await client.collection('posts').create({ title: 'x' });      // ✓
await client.collection('posts').create({ nope: 1 });          // ✗ compile error
```

> **Dynamic collection names are fully supported.** The typed client accepts any
> runtime string for `collection(name)` and still returns the typed service for
> known collection names. So route params and dynamic lookups work naturally:
>
> ```typescript
> function load(name: string) {
>  return client.collection(name).getList(); // ✓ works for any string
> }
> ```

### 2. Hand-written generics (no codegen)

Pass a record interface to `collection<T>()` or use `.typed<T>()`:

```typescript
interface Post {
  id: string;
  title: string;
  published: boolean;
}

const postsSvc = client.collection('posts').typed<Post>();
const post = await postsSvc.getOne('abc123'); // post.title: string

await postsSvc.create({ title: 'Hi', published: true }); // ✓
await postsSvc.create({ title: 'Hi', nope: 1 });          // ✗ compile error
```

### 3. Runtime schema types (experimental)

Fetch schemas at runtime and let the client derive field types:

```typescript
const res = await fetch('http://localhost:4000/api/collections', {
  headers: { Authorization: 'Bearer ' + token },
});
const { items } = await res.json(); // CollectionSchema[]

const client = new LazypockClient({
  baseUrl: 'http://localhost:4000/api',
  types: { schemas: items },
});

const code = client.generateTypes(); // string — write to lazypock.types.ts
```

### CLI reference

```bash
lazypock [options]

Options:
  --url <url>        API base URL (or LAZYPOCK_URL)
  --apikey <key>    API key (or LAZYPOCK_API_KEY) — recommended, no login round-trip
  --api-key <key>   Deprecated alias for --apikey
  --email <email>    Superuser email (or LAZYPOCK_EMAIL)
  --password <pw>    Superuser password (or LAZYPOCK_PASSWORD)
  --output <file>   Output file (default: lazypock.types.ts)
  --out <file>      Deprecated alias for --output
  --package <name>   Package name to import (default: lazypock)
  --skip-system      Skip system collections
```

> **Note:** `lazypock-gen` is still available as a deprecated alias.

You must provide credentials one of two ways (or via the matching env vars):

1. `--apikey` / `LAZYPOCK_API_KEY` — scoped to collection listing, no login.
2. `--email` + `--password` / `LAZYPOCK_EMAIL` + `LAZYPOCK_PASSWORD` — superuser login.

```


## API Reference

### LazypockClient

The main client class.

#### Constructor Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `baseUrl` | `string` | required | API base URL (e.g. `http://localhost:4000/api`) |
| `storage` | `StorageAdapter` | `memoryStorage` | Custom storage adapter for token persistence |
| `authStore` | `AuthStore` | auto-created | Explicit auth store instance |
| `realtime` | `RealtimeService` | auto-created | Real-time service for WebSocket subscriptions |

#### Authentication Methods

- `login(email, password, collection?)` — Login as superuser or auth collection user
- `authWithPassword(collection, identity, password, options?)` — Auth collection login
- `authRefresh(collection, options?)` — Refresh auth token
- `checkSuperuser()` — Check if any superuser exists
- `setup(email, password)` — Create initial superuser
- `logout()` — Clear auth state
- `me(options?)` — Get current superuser profile

#### Collections Service (`client.collections`)

PocketBase-style service for the collections themselves (admin):

- `collections.getList(params?)` — Paginated list of collections
- `collections.getFullList(options?)` — Fetch all collections (auto-paginates)
- `collections.getOne(id, options?)` — Get collection by ID/name
- `collections.create(data, options?)` — Create collection
- `collections.update(id, data, options?)` — Update collection
- `collections.delete(id, options?)` — Delete collection
- `collections.subscribe(cb)` — Subscribe to collection create/update/delete events (returns unsubscribe fn)
- `collections.unsubscribe()` — Unsubscribe from registry events
#### File Operations

- `files.upload(file, filename?, options?, meta?)` — Upload a file
- `files.getUrl(fileId)` — Get file metadata
- `files.delete(fileId, options?)` — Delete a file
- `getFileUrl(baseUrl, fileId)` — Construct a file URL from base URL and file ID (utility)

#### Realtime

- `realtime.connect(opts)` — Connect to WebSocket
- `realtime.disconnect()` — Disconnect
- `realtime.subscribe(topic, callback)` — Low-level subscribe (topic like `collection:posts`)
- `realtime.unsubscribe(topic, callback?)` — Low-level unsubscribe
- `collection(name).subscribe(callback, recordId?)` — Subscribe to record changes; callback receives `{ action, record }`; returns unsubscribe fn
- `collection(name).unsubscribe(recordId?)` — Unsubscribe from record changes

### CollectionService

Returned by `client.collection(name)`.

- `getList(page, perPage, options?)` — Paginated list of records
- `getFullList(options?)` — Fetch all records (auto-paginates)
- `getFirstListItem(filter, options?)` — Fetch first record matching filter
- `getOne(id, options?)` — Get record by ID
- `create(data, options?)` — Create record
- `update(id, data, options?)` — Update record
- `delete(id, options?)` — Delete record
- `subscribe(callback, recordId?)` — Subscribe to record changes (PocketBase-style)
- `unsubscribe(recordId?)` — Unsubscribe
- `authWithPassword(identity, password, options?)` — Login to this auth collection
- `authRefresh(options?)` — Refresh token for this auth collection
- `authMethods(options?)` — Get available auth methods

### AuthStore

Handles token persistence and auto-refresh.

- `token` — Current JWT token
- `model` — Current auth model (user record or null)
- `isValid` — Whether a token exists
- `isExpired` — Whether the current token has expired (with 30s buffer)
- `collectionName` — Name of the auth collection used for token refresh
- `set(token, model)` — Update token and model
- `setCollectionName(name)` — Set the auth collection name for token refresh
- `clear()` — Clear all auth state
- `onChange(callback)` — Listen for auth changes (returns unsubscribe function)
- `init()` — Restore persisted auth from storage

### Types

```typescript
interface ApiRecord {
  id: string;
  collectionId: string;
  collectionName: string;
  created: string;
  updated: string;
  [key: string]: unknown;
}

interface ListResult<T> {
  page: number;
  perPage: number;
  totalItems: number;
  totalPages: number;
  items: T[];
}

interface AuthModel {
  id: string;
  [key: string]: unknown;
}

interface FileRecord {
  id: string;
  filename: string;
  mimeType: string;
  size: number;
  url: string;
  [key: string]: unknown;
}

interface RequestOptions {
  signal?: AbortSignal;
  fetch?: typeof fetch;
  headers?: Record<string, string>;
}
```

## Error Handling

The SDK throws `ApiError` on non-2xx responses:

```typescript
import { LazypockClient, ApiError } from 'lazypock';

try {
  await client.collection('posts').create({ title: 'My Post' });
} catch (err) {
  if (err instanceof ApiError) {
    console.log(err.status);    // HTTP status code
    console.log(err.message);   // Error message
    console.log(err.data);      // Full response data
  }
}
```

## Configuration

### Storage Adapter

By default, the SDK uses `localStorage` for token persistence. You can provide a custom adapter:

```typescript
import { LazypockClient, AuthStore } from 'lazypock';

const customStorage = {
  get: async (key) => await AsyncStorage.getItem(key),
  set: async (key, value) => await AsyncStorage.setItem(key, value),
  remove: async (key) => await AsyncStorage.removeItem(key),
};

const client = new LazypockClient({
  baseUrl: 'http://localhost:4000/api',
  storage: customStorage,
});
```

### Auto Token Refresh

The SDK automatically refreshes expired auth tokens. When a token expires, the next API call triggers a transparent refresh via the `auth-refresh` endpoint. No manual intervention needed.

## Real-time Subscriptions

```typescript
// Subscribe to all changes in a collection (PocketBase-style: callback-first)
const off = client.collection('posts').subscribe((event) => {
  console.log(event.action); // 'create' | 'update' | 'delete'
  console.log(event.record);
});

// Subscribe to a specific record only
client.collection('posts').subscribe((event) => { ... }, 'abc123');

// Unsubscribe
client.collection('posts').unsubscribe();

// ...or call the returned unsubscribe function for one-shot listeners:
off();
```

### Anonymous / rule-based realtime

Realtime subscriptions honor your API **and list rules** — matching PocketBase
behavior. This means **non-logged-in users can subscribe** to collections whose
list rules are public (empty `""` string) or anon-friendly
(`@request.auth.*` filters). The SDK auto-connects the WebSocket on first use,
so no token is required to receive public change events:

```typescript
// Works without logging in, as long as the collection's list rule allows it
const off = client.collection('public_feed').subscribe((e) => {
  console.log(e.action, e.record);
});
```

## License

[MIT](LICENSE) © 2024-2025 Chris Nguyen (gnuzd)
