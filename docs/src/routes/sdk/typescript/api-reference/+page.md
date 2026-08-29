---
title: API Reference
---

# API Reference

## LazypockClient

The main client class.

### Constructor Options

| Option      | Type           | Default        | Description                                                             |
| ----------- | -------------- | -------------- | ----------------------------------------------------------------------- |
| `baseUrl`   | `string`       | required       | API base URL (e.g. `http://localhost:4000/api`)                         |
| `storage`   | `StorageAdapter` | `memoryStorage` | Custom storage adapter for token persistence                            |
| `authStore` | `AuthStore`    | auto-created   | Explicit auth store instance                                            |
| `realtime`  | `RealtimeService` | auto-created | Real-time service for WebSocket subscriptions                           |

## Collections Service (`client.collections`)

PocketBase-style service for the collections themselves (admin):

- `collections.getList(params?)` — Paginated list of collections
- `collections.getFullList(options?)` — Fetch all collections (auto-paginates)
- `collections.getOne(id, options?)` — Get collection by ID/name
- `collections.create(data, options?)` — Create collection
- `collections.update(id, data, options?)` — Update collection
- `collections.delete(id, options?)` — Delete collection
- `collections.subscribe(cb)` — Subscribe to collection create/update/delete events (returns unsubscribe fn)
- `collections.unsubscribe()` — Unsubscribe from registry events

## CollectionService

Returned by `client.collection(name)`.

- `select(...fields)` — Project reads to the given fields (see [Queries](/sdk/typescript/queries));
  `select('*')` restores the all-visible default
- `getList(page, perPage, options?)` — Paginated list of records (typed `filter`/`sort`/`expand`/`fields`)
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

## Types

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

## Auto Cancellation

The SDK auto-cancels duplicated pending requests for you (PocketBase-compatible behaviour). When a new
request is issued with the same request key as a still-pending request, the previous one is aborted —
only the last request executes:

```typescript
// Only the last call will execute; the first two are auto-cancelled
await client.collection('posts').getList(1, 20); // cancelled
await client.collection('posts').getList(2, 20); // cancelled
await client.collection('posts').getList(3, 20); // executed
```

By default the request key is `HTTP_METHOD + path` (e.g. `"GET /api/posts?page=1"`), so duplicate
calls with identical URLs cancel each other. Cancelled requests reject with an `ApiError` whose
`isAbort` is `true`:

```typescript
try {
  await client.collection('posts').getList(1, 20);
} catch (err) {
  if (err instanceof ApiError && err.isAbort) {
    // superseded by a newer request — safe to ignore
  }
}
```

### Per-request control

Pass `requestKey` in the request options to customize the key, or disable auto-cancellation for a
specific request:

```typescript
await client.collection('posts').getList(1, 20, { requestKey: 'my-list' }); // cancelled
await client.collection('posts').getList(1, 20, { requestKey: 'my-list' }); // executed

await client.collection('posts').getList(1, 20, { requestKey: null }); // executed
await client.collection('posts').getList(1, 20, { requestKey: null }); // executed
```

### Global control

```typescript
// Disable auto-cancellation globally
client.autoCancellation(false);

// Manually cancel pending requests
client.cancelRequest('GET /api/posts?page=1');
client.cancelAllRequests();
```

### Single-flight dedup (getFullList)

`getFullList()` (and `collections.getFullList()`) are **single-flight**: concurrent calls with the
same effective options share one in-flight request instead of firing duplicates. This means the
common pattern below results in **one** network request, and **both** callers resolve with the same
data — no abort rejection:

```typescript
const [a, b] = await Promise.all([
  client.collection('posts').getFullList(),
  client.collection('posts').getFullList()
]);
// one GET fired; a === b
```

Calls with **different** options (e.g. different `sort`/`filter`) are still distinct requests.
Multi-page fetches continue to work normally — each page request is unique (page number is part of
the URL), so pages never cancel each other.

The underlying `singleFlight` option is also available on any request when you want to coalesce
concurrent identical calls yourself:

```typescript
await client.collection('posts').getList(1, 20, { singleFlight: true });
```

## Error Handling

The SDK throws `ApiError` on non-2xx responses:

```typescript
import { LazypockClient, ApiError } from 'lazypock';

try {
  await client.collection('posts').create({ title: 'My Post' });
} catch (err) {
  if (err instanceof ApiError) {
    console.log(err.status); // HTTP status code
    console.log(err.message); // Error message
    console.log(err.data); // Full response data
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
  remove: async (key) => await AsyncStorage.removeItem(key)
};

const client = new LazypockClient({
  baseUrl: 'http://localhost:4000/api',
  storage: customStorage
});
```

### Auto Token Refresh

The SDK automatically refreshes expired auth tokens. When a token expires, the next API call triggers
a transparent refresh via the `auth-refresh` endpoint. No manual intervention needed.
