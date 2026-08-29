---
title: Type Safety
---

# Type Safety

The SDK offers three levels of type safety — pick what fits your project.

## 1. Fully typed via codegen (recommended)

Connect to your API once and generate a typed client — every collection becomes an interface with the
exact field types from your schema (selects become string unions, relations become record IDs, etc.).

```bash
npx lazypock \
  --url http://localhost:4000/api \
  --email admin@example.com \
  --password your-password
# writes ./lazypock.types.ts
```

> `lazypock-gen` remains as a deprecated alias for backwards compatibility — the canonical command is
> now simply `lazypock`.

**Use an API key instead of a password** (recommended). Generate one from the Studio
_Settings → API Keys_ dashboard, then:

```bash
npx lazypock --url http://localhost:4000/api --apikey lazypock_xxxxxxxx
# or via env: LAZYPOCK_URL=... LAZYPOCK_API_KEY=... npx lazypock
```

API keys are stored as a SHA-256 hash (raw value shown once at generation) and are scoped to
collection listing — ideal for codegen (they can `GET /collections` without a login round-trip, and
cannot read or mutate your records).

Then in your app:

```typescript
import { createClient } from './lazypock.types';

const client = createClient({ baseUrl: 'http://localhost:4000/api' });
await client.login('admin@example.com', 'password');

// Collection access is fully type-checked:
const post = await client.collection('posts').getOne('abc123');
// post.title — string, post.published — boolean, …

await client.collection('posts').create({ title: 'x' }); // ✓
await client.collection('posts').create({ nope: 1 }); // ✗ compile error
```

> **Dynamic collection names are fully supported.** The typed client accepts any runtime string for
> `collection(name)` and still returns the typed service for known collection names. So route params
> and dynamic lookups work naturally:
>
> ```typescript
> function load(name: string) {
>   return client.collection(name).getList(); // ✓ works for any string
> }
> ```

## 2. Hand-written generics (no codegen)

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
await postsSvc.create({ title: 'Hi', nope: 1 }); // ✗ compile error
```

## 3. Runtime schema types (experimental)

Fetch schemas at runtime and let the client derive field types:

```typescript
const res = await fetch('http://localhost:4000/api/collections', {
  headers: { Authorization: 'Bearer ' + token }
});
const { items } = await res.json(); // CollectionSchema[]

const client = new LazypockClient({
  baseUrl: 'http://localhost:4000/api',
  types: { schemas: items }
});

const code = client.generateTypes(); // string — write to lazypock.types.ts
```

The codegen CLI emits a `lazypockSchema` snapshot next to the types, and the generated `createClient()`
wires it in automatically — so the schema-driven behaviour below (hidden-field exclusion, query
validation) works out of the box.

## CLI reference

```
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

You must provide credentials one of two ways (or via the matching env vars):

1. `--apikey` / `LAZYPOCK_API_KEY` — scoped to collection listing, no login.
2. `--email` + `--password` / matching env vars — superuser login.
