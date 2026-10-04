---
title: TypeScript SDK
---

# TypeScript SDK

The official TypeScript client for Lazypock — fully typed via codegen, PocketBase-compatible API surface, and runtime schema support.

<h2 id="install">Install</h2>

```bash
npm install lazypock
```

bun / pnpm / yarn work the same way.

> **Needs a running Lazypock server.** The SDK talks to a Lazypock backend over its REST API.
> No server yet? See the [Server Guide](/server) — the quickest path is Docker + a prebuilt binary.

### Build from source

```bash
git clone git@github.com:gnuzd/lazypock-ts.git
cd lazypock-ts
npm install
npm run build
```

---

<h2 id="quick-start">Quick Start</h2>

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

// Auth collections — create a user (password is optional + write-only)
const user = await client.collection('users').create({
  email: 'ada@example.com',
  password: 'correct-horse-battery' // hashed server-side, never returned
});
const session = await client.authWithPassword('users', 'ada@example.com', 'correct-horse-battery');
// session.token — stored in client.authStore for subsequent requests

// File upload
const file = await client.files.upload(fileInput.files[0]);

// Real-time subscriptions (PocketBase-style: callback-first)
client.collection('posts').subscribe((e) => console.log(e.action, e.record));
```

### Next steps

- [Type Safety](/sdk/typescript#type-safety) — codegen a fully typed client
- [Queries](/sdk/typescript#queries) — `select()`, filters, sort, expand
- [Realtime](/sdk/typescript#realtime) — live subscriptions
- [Files](/sdk/typescript#files) — uploads and file URLs
- [Auth](/sdk/typescript#auth) — auth collections and token handling

---

<h2 id="type-safety">Type Safety</h2>

The SDK offers three levels of type safety — pick what fits your project.

### 1. Fully typed via codegen (recommended)

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
await postsSvc.create({ title: 'Hi', nope: 1 }); // ✗ compile error
```

### 3. Runtime schema types (experimental)

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

### Query autocomplete

On a typed service, `filter` / `sort` / `expand` are validated at compile time.
For per-field **autocomplete**, use the array forms of `sort`/`expand` and
the typed filter builder:

```typescript
const q = client.collection('posts').where;

client.collection('posts').getList(1, 20, { sort: ['-created'] });
client.collection('posts').getList(1, 20, { expand: ['author.name'] });
client.collection('posts').getList(1, 20, {
  filter: q('title').contains('x').and(q('published').eq(true))
});
```

- `sort: ['-created']` — the editor suggests each field (`-created`, `title`, …).
- `expand: ['author.name']` — relation fields are suggested.
- `q('title')` — field names are suggested, operators are methods, and
  values are escaped for you.

See [Queries](/sdk/typescript/queries) for the full guide.

### CLI reference

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

---

<h2 id="queries">Queries</h2>

Everything you can pass to `getList`, `getFullList`, `getFirstListItem`, and
`getOne`: **sorting**, **filtering**, **relation expansion**, and **field
projection**. Each section starts with the simplest form and ends with the
typed helpers.

> **New here?** Use the [recipes](#recipes) at the bottom for the most common
> tasks (fetch by ids, search, filter by relation, …).

### Quick reference

Throughout this guide `q` is the typed filter builder:

```typescript
const q = postsSvc.where;
```

| I want to… | Do this |
| --- | --- |
| Sort newest first | `getFullList({ sort: ['-created'] })` |
| Sort by two fields | `getList(1, 20, { sort: ['-published', 'title'] })` |
| Filter by a list of ids | `getFullList({ filter: q('id').in(ids) })` |
| Search a text field | `getList(1, 20, { filter: q('title').contains(term) })` |
| Filter by relation id | `getFullList({ filter: q('author').eq(userId) })` |
| Combine conditions (AND) | `q('published').eq(true).and(q('views').gt(100))` |
| Either of two conditions (OR) | `q('a').eq(1).or(q('b').eq(2))` |
| Negate a condition | `q('archived').eq(true).not()` |
| Also fetch the related record | `getList(1, 20, { expand: ['author'] })` |
| Only some fields of the related record | `getList(1, 20, { expand: ['author.name', 'author.email'] })` |
| Return only some fields | `client.collection('posts').select('id', 'title').getList()` |

Every option works with the raw string form too — the builder is a
convenience that checks field names and escapes values for you.

---

### Sorting

`sort` accepts a comma-separated string or an **array**. `-` means descending,
a bare field (or `+field`) means ascending.

```typescript
// String form
await postsSvc.getList(1, 20, { sort: '-created' });
await postsSvc.getList(1, 20, { sort: 'title,-published' });

// Array form — one entry per field (recommended: the editor suggests each one)
await postsSvc.getList(1, 20, { sort: ['-created'] });
await postsSvc.getList(1, 20, { sort: ['title', '-published'] });
```

| Form | Editor suggests fields? | Validated? |
| --- | --- | --- |
| `sort: '-title,published'` (string) | No (only the whole string) | ✅ every token |
| `sort: ['-title', 'published']` (array) | ✅ each entry | ✅ each entry |

Unknown fields are a **compile error** on a typed service:

```typescript
await postsSvc.getList(1, 20, { sort: ['-nope'] }); // ✗ compile error
```

---

### Filtering

There are two ways to build a filter:

1. **Filter expression string** — full PocketBase syntax, validated at compile
   time on typed services. Best for dynamic strings and advanced expressions.
2. **Typed builder** (`service.where(field)`) — field names are suggested,
   operators are methods, and values are escaped automatically. Best for
   hand-written queries.

Both produce the same thing and can be mixed: pass either as `filter`.
For one-off filters you can also build inline with a callback — see
[Inline filter callback](#inline-filter-callback).

#### Filter expressions (string)

The syntax is `field operator value`, combined with `&&` (and), `||` (or),
`!` (not), and parentheses.

```typescript
await postsSvc.getList(1, 20, { filter: "title ~ 'hello'" });
await postsSvc.getList(1, 20, { filter: "published = true" });
await postsSvc.getList(1, 20, { filter: "views >= 100" });
await postsSvc.getList(1, 20, { filter: "title ~ 'a' && published = true" });
await postsSvc.getList(1, 20, { filter: "(title = 'a' || title = 'b')" });
await postsSvc.getList(1, 20, { filter: "author.email = 'ada@example.com'" });
await postsSvc.getList(1, 20, { filter: "deleted_at = null" }); // IS NULL
```

> **Relation dot-paths** (`author.email = 'x'`, including multi-level paths
> and multi-relations) and **null checks** (`field = null` /
> `field != null`) are supported by current LazyPock servers. On an older
> server a dot-path filter is rejected with `400 Invalid filter expression`;
> filter by the relation id (`author = 'USER_ID'`) instead.

##### Operators

| Operator | Meaning | Example |
| --- | --- | --- |
| `=` | equal | `status = 'open'` |
| `!=` | not equal | `status != 'closed'` |
| `~` | contains (LIKE) | `title ~ 'hello'` |
| `!~` | does not contain | `title !~ 'draft'` |
| `>` `>=` `<` `<=` | comparisons | `views >= 100` |
| `?=` | any array element equals | `tags ?= 'news'` |
| `?!=` | any array element differs | `tags ?!= 'news'` |
| `?~` | any array element contains | `tags ?~ 'new'` |
| `?!~` | any array element does not contain | `tags ?!~ 'new'` |
| `?>` `?>=` `?<` `?<=` | any array element compares | `scores ?> 10` |

Strings use single or double quotes. Values containing `&&`, `||`, or quotes
must be quoted (`title ~ 'a && b'`). On a typed service **every clause** is
checked — a typo in any field or operator is a compile error:

```typescript
await postsSvc.getList(1, 20, { filter: "title = 'a' && nope = 'b'" }); // ✗ compile error
```

#### The typed filter builder

Start a clause with `service.where(field)`, pick an operator method, then
combine expressions. Field names are suggested from the collection's schema,
and values are quoted/escaped by the builder.

```typescript
const q = postsSvc.where;

// one clause
await postsSvc.getList(1, 20, { filter: q('title').contains('hello') });

// combine
await postsSvc.getList(1, 20, {
  filter: q('title').contains('hello').and(q('published').eq(true)),
});
```

##### Comparison methods

| Method | Emits | Notes |
| --- | --- | --- |
| `eq(v)` | `field = v` | use `eq(null)` for *is empty* |
| `neq(v)` | `field != v` | |
| `contains(v)` | `field ~ v` | text search |
| `notContains(v)` | `field !~ v` | |
| `gt(v)` / `gte(v)` | `field > v` / `field >= v` | |
| `lt(v)` / `lte(v)` | `field < v` / `field <= v` | |
| `in(values)` | `(field = a \|\| field = b \|\| …)` | **list membership** |
| `notIn(values)` | `(field != a && field != b && …)` | |

##### Array ("any element") methods

For multi-select / multiple-relation / multiple-file fields, prefix with `any`:

| Method | Emits |
| --- | --- |
| `anyEq(v)` | `field ?= v` |
| `anyNeq(v)` | `field ?!= v` |
| `anyContains(v)` | `field ?~ v` |
| `anyNotContains(v)` | `field ?!~ v` |
| `anyGt(v)` / `anyGte(v)` | `field ?> v` / `field ?>= v` |
| `anyLt(v)` / `anyLte(v)` | `field ?< v` / `field ?<= v` |

##### Combining expressions

| Method | Emits | Meaning |
| --- | --- | --- |
| `.and(other)` | `(a && b)` | both must match |
| `.or(other)` | `(a \|\| b)` | either may match |
| `.not()` | `!(a)` | invert |
| `.toString()` | — | the raw filter string |

Combinations are parenthesised, so chaining never introduces precedence
surprises:

```typescript
const filter = q('title').contains('x').and(q('published').eq(true));
// → (title ~ 'x' && published = true)

await postsSvc.getList(1, 20, {
  filter: q('status').eq('open').or(q('status').eq('pending')).not(),
});
// → !((status = 'open' || status = 'pending'))
```

##### Values and escaping

Values may be `string`, `number`, `boolean`, or `null`. Strings are
single-quoted and escaped for you, so inputs like `it's` are safe:

```typescript
q('title').eq("it's"); // → title = 'it\'s'
```

The builder checks values as filter scalars; the server enforces the exact
per-field type. An empty `in([])` / `notIn([])` throws (it can never match —
that is usually a bug).

##### Mixing builder and string

A builder expression can be passed wherever a filter string is accepted, and
you can still use the raw string for advanced cases:

```typescript
await postsSvc.getFirstListItem(q('slug').eq('hello-world'));
await postsSvc.getList(1, 20, { filter: "title ~ 'x' && published = true" });
```

##### Inline filter callback

For a one-off filter, pass a callback instead of binding `where` to a
variable. The callback receives the same typed `where` helper, so field names
are still suggested and checked:

```typescript
await postsSvc.getFullList({
  filter: (w) => w('title').contains('x').and(w('published').eq(true)),
});

await postsSvc.getFirstListItem((w) => w('slug').eq('hello-world'));
```

This is equivalent to the `q` form and works anywhere `filter` is accepted
(`getList`, `getFullList`, `getFirstListItem`).

---

### Expanding relations

`expand` fetches the referenced records and attaches them under
`record.expand` instead of leaving just the id. It accepts a comma-separated
string or an array.

```typescript
const post = await postsSvc.getOne('abc123', { expand: 'author' });
post.expand?.author?.email; // the full related record

// array form (per-token autocomplete)
const posts = await postsSvc.getFullList({ expand: ['author', 'category'] });
```

`record.author` stays the relation id; the related record is on
`record.expand.author`.

#### Nested relations

Use a dot-path to expand a relation *of* a relation:

```typescript
await postsSvc.getFullList({ expand: ['author.profile'] });
// post.expand.author.expand.profile
```

#### Only some fields of the expanded record

Add the field(s) after the relation:

```typescript
await postsSvc.getFullList({ expand: ['author.name', 'author.email'] });
// post.expand.author === { name: '…', email: '…' }  (no other fields)

// combine a full expansion with a narrowed one
await postsSvc.getFullList({ expand: ['author', 'category.name'] });
```

The SDK applies this selection itself, so it works the same against every
server: PocketBase narrows it server-side, the LazyPock server returns the
full related record and the SDK keeps only the requested fields.

> **How this works:** `expand` only understands *relations* —
> `expand=author.name` on its own is ignored. The SDK detects that `name` is
> not a relation, asks for the `author` relation instead, and then keeps only
> the requested fields.
>
> Distinguishing "field on the relation" from "nested relation" needs the
> target collection's schema. The codegen `createClient()` wires schemas in
> automatically; with a hand-written client, pass `types.schemas`. Without a
> schema, two or more dotted tokens under one relation are treated as a field
> selection, and a lone dotted token stays a nested relation (with a warning).

#### Typed results

With a codegen/typed service, `record.expand.author` is typed as the target
collection's record, so `post.expand?.author?.email` autocompletes instead of
being `unknown`.

#### Expanded records are never dropped

If you also project fields (`select(...)` or an explicit `fields`), the
`expand.*` entries are merged into the projection, so the server's strict
`fields` filter can't silently remove the expanded data.

---

### Projecting fields

`select(...)` limits the fields returned by reads (PocketBase `fields`). It
returns a derived service — the original is untouched.

```typescript
const slim = client.collection('posts').select('id', 'title');
const list = await slim.getList(1, 20);
// GET /api/posts?fields=id,title

await slim.getOne('abc123');
```

- `select('*')` (or no `select()` call) requests all visible fields.
- `select()` with no arguments resets to the default.
- When a schema is known, hidden fields are excluded from responses.
- An explicit `fields` option overrides the `select()` preset for that call.

---

### Recipes

#### Fetch records by a list of ids

```typescript
const q = postsSvc.where;
const posts = await postsSvc.getFullList({ filter: q('id').in(ids) });
```

`in()` builds the `(id = 'a' || id = 'b' || …)` expression PocketBase needs.

#### Search a text field

```typescript
const q = postsSvc.where;
await postsSvc.getFullList({ filter: q('title').contains(search) });
// or: filter: `title ~ '${search.replaceAll("'", "\\'")}'`  — the builder escapes for you
```

#### Filter by relation, and show the related record

```typescript
const q = postsSvc.where;
const posts = await postsSvc.getFullList({
  filter: q('author').eq(userId),
  expand: ['author.name'],
});
```

> To filter on a field *of* the related record, use a relation dot-path —
> `q('author.email').eq(email)` or `"author.email = 'x'"`. This compiles to a
> correlated subquery (requires a server with relation dot-path support).
> `field = null` / `field != null` select empty / non-empty values.

#### Filter by status, newest first, expand the author

```typescript
const q = postsSvc.where;
await postsSvc.getList(1, 20, {
  filter: q('status').eq('published').and(q('views').gte(100)),
  sort: ['-created'],
  expand: ['author.name'],
});
```

#### Unpublished drafts, excluding archived

```typescript
const q = postsSvc.where;
await postsSvc.getFullList({
  filter: q('published').eq(false).and(q('archived').eq(true).not()),
});
```

#### Any tag matches

```typescript
await postsSvc.getList(1, 20, { filter: "tags ?= 'news'" });
// or with the builder:
await postsSvc.getList(1, 20, { filter: postsSvc.where('tags').anyEq('news') });
```

---

<h2 id="realtime">Realtime</h2>

Subscribe to live record changes — PocketBase-style, callback-first.

```typescript
// Subscribe to all changes in a collection
const off = client.collection('posts').subscribe((event) => {
  console.log(event.action); // 'create' | 'update' | 'delete'
  console.log(event.record);
});

// Subscribe to a specific record only
client.collection('posts').subscribe((event) => { /* ... */ }, 'abc123');

// Unsubscribe
client.collection('posts').unsubscribe();

// ...or call the returned unsubscribe function for one-shot listeners:
off();
```

### Anonymous / rule-based realtime

Realtime subscriptions honor your API **and list rules** — matching PocketBase behavior. This means
**non-logged-in users can subscribe** to collections whose list rules are public (empty `""` string)
or anon-friendly (`@request.auth.*` filters). The SDK auto-connects the WebSocket on first use, so no
token is required to receive public change events:

```typescript
// Works without logging in, as long as the collection's list rule allows it
const off = client.collection('public_feed').subscribe((e) => {
  console.log(e.action, e.record);
});
```

### Low-level realtime service

For advanced use cases you can talk to the underlying service directly:

- `realtime.connect(opts)` — Connect to WebSocket
- `realtime.disconnect()` — Disconnect
- `realtime.subscribe(topic, callback)` — Low-level subscribe (topic like `collection:posts`)
- `realtime.unsubscribe(topic, callback?)` — Low-level unsubscribe

---

<h2 id="files">Files</h2>

Upload, list, delete and build URLs for file records. Richtext/Markdown fields store plain URLs, so
the same helpers work for images embedded in content.

```typescript
// Upload a file (through the app)
const file = await client.files.upload(input.files[0], undefined, undefined, {
  collectionName: 'posts',
  fieldName: 'cover',
  origin: 'library', // field | editor | library
  variants: ['content'] // generate this preset before the response
});

// Direct-to-bucket upload (S3/R2): presign → PUT → verify
const direct = await client.files.uploadDirect(input.files[0], {
  collectionName: 'posts',
  fieldName: 'cover'
});

// List the library (superuser)
const { items, total } = await client.files.list({ mime: 'image/', q: 'sunset', page: 1 });

// Delete one
await client.files.delete(file!.id);
```

### Upload methods

| Method | Description |
| --- | --- |
| `upload(file, filename?, options?, meta?)` | Multipart upload through the app. `meta` accepts `collectionName`, `recordId`, `fieldName`, `origin` (`field` \| `editor` \| `library`) and `variants` |
| `uploadDirect(file, opts?)` | Presign → `PUT` straight to the bucket → verify. Falls back to `upload()` when the server has no direct upload |
| `presign({ filename, size, mime, ... })` | Step 1 on its own: returns `{ id, key, method, url, headers }` |
| `complete(fileId, { variant? })` | Step 2 on its own: verify the uploaded object |
| `list({ mime?, q?, page?, perPage? })` | Library listing (superuser) |
| `delete(fileId)` | Delete the file and its variants (superuser) |

### URLs

| Helper | Result |
| --- | --- |
| `getFileUrl(baseUrl, fileId)` | The original: `/api/files/<id>` |
| `getThumbUrl(baseUrl, fileId, size)` | Legacy thumbnail: `/thumbs/<size>` |
| `getScaleUrl(baseUrl, fileId, size)` | Arbitrary size: `/scale/300x200` |
| `getVariantUrl(baseUrl, fileId, preset)` | Named preset: `/scale/content` |

### File records

A `FileRecord` carries ready-made URLs, so you rarely build them by hand:

```json
{
  "id": "1f0c…",
  "filename": "sunset.png",
  "mimeType": "image/png",
  "size": 482913,
  "url": "/api/files/1f0c…",
  "thumbs": { "100x100": "/api/files/1f0c…/thumbs/100x100" },
  "variants": {
    "thumb": "/api/files/1f0c…/scale/thumb",
    "content": "https://cdn.example.com/lazypock/1f0c…/content.webp"
  }
}
```

> Embedded images should use the `content` variant (`getVariantUrl(url, id, 'content')`), never the
> original — that is what the Studio editor inserts. See
> [File storage & images](https://lazypock.gnuzd.dev/files) for presets, storage backends and
> presigned uploads.

### Utilities

- `getFileUrl(baseUrl, fileId)` — Construct a file URL from base URL and file ID (utility).

---

<h2 id="auth">Auth</h2>

### Authentication methods

- `login(email, password, collection?)` — Login as superuser or auth collection user
- `authWithPassword(collection, identity, password, options?)` — Auth collection login
- `authRefresh(collection, options?)` — Refresh auth token
- `checkSuperuser()` — Check if any superuser exists
- `setup(email, password)` — Create initial superuser
- `logout()` — Clear auth state
- `me(options?)` — Get current superuser profile

### Auth collections

Collections can be **base** (`type: "base"`, plain records) or **auth** (`type: "auth"`, accounts —
the built-in `users` collection is an auth collection). Auth collections have an email field and a
write-only password field, plus system fields (`verified`, `emailVisibility`).

The `password` field is **write-only**:

- **Hidden** — never returned by the server, never shown in the Studio record browser, and omitted
  from the generated **read model** (`UsersRecord`).
- **Optional** — accounts may exist without a password (e.g. OAuth-only users or invite flows), so
  `create()` typechecks without it.

```typescript
// Create a user (password optional + write-only)
const user = await client.collection('users').create({
  email: 'ada@example.com',
  password: 'correct-horse-battery' // hashed server-side, never returned
});

// Login to an auth collection
const session = await client.authWithPassword('users', 'ada@example.com', 'correct-horse-battery');
// session.token — stored in client.authStore for subsequent requests
```

### OAuth2

Sign in with an OAuth2 provider (Google, GitHub, Apple, …) on an auth collection.

#### Popup flow (web)

```typescript
const auth = await client.collection('users').authWithOAuth2({ provider: 'google' });
// auth.token + auth.record + auth.meta (isNew, email, avatarURL, …)
// client.authStore is populated when the promise resolves
```

One call handles the whole flow: it fetches the provider's authorization URL,
opens a popup, receives the single-use authorization `code` the backend relays
via `postMessage`, exchanges it, and populates the auth store — the same result
shape as `authWithPassword`. `createData` is forwarded on first sign-up.

Options:

- `provider` (required) — the provider name, e.g. `'google'`
- `createData` — extra fields merged into the record on first sign-up
- `urlCallback(url)` — called with the authorization URL instead of opening a
  popup (the presented window must preserve `window.opener`)
- `popup: { width, height }` — popup geometry (default 500×700)
- `timeoutMs` — abandon after this long (default 120000)

#### Direct code exchange (mobile / non-browser)

The popup flow depends on `window.postMessage`, so on React Native (or when you
present the URL yourself), capture the `code` from your redirect and exchange it:

```typescript
const methods = await client.collection('users').listAuthMethods();
const google = methods?.oauth2.providers.find((p) => p.name === 'google');
// …present google.authURL (expo-web-browser, ASWebAuthenticationSession, …)
// …capture the redirect `code` via your deep link, then:
const auth = await client.collection('users').authWithOAuth2Code({
  provider: 'google',
  code,
  codeVerifier: google.codeVerifier,
  createData: { /* extra fields on first sign-up */ },
});
```

#### Available providers

```typescript
const methods = await client.collection('users').listAuthMethods();
// methods.oauth2.providers → [{ name, authURL, state, codeVerifier }]
```

#### Notes

- The backend creates the PKCE `state`/`codeVerifier` and validates the pending
  session on the redirect. The popup page relays **only the single-use
  authorization `code`** (never a token or user record); the SDK then exchanges
  it via `authWithOAuth2Code`.
- The `codeVerifier` returned by `listAuthMethods()` is the PKCE verifier, not a
  provider secret — `client_secret` never leaves the backend.
- The popup flow posts the result back to the origin that started it, so it also
  works when the API and the app are on different origins (as long as the app's
  origin is in the server's allowed origins / `LAZYPOCK_CORS_ORIGINS`).

### AuthStore

Handles token persistence and auto-refresh.

- `token` — Current JWT token
- `model` — Current auth model (user record or null)
- `isValid` — Whether a token exists
- `isExpired` — Whether the current token has expired (with 30s buffer)
- `collectionName` — Name of the auth collection for this session (`null` for superuser sessions)
- `set(token, model)` — Update token and model
- `setCollectionName(name)` — Set the auth collection name for token refresh
- `clear()` — Clear all auth state
- `onChange(callback)` — Listen for auth changes (returns unsubscribe function)
- `init()` — Restore persisted auth from storage

`collectionName` is **persisted with the token and model**, so it survives a page reload: signing in
with an auth collection, calling `client.authStore.init()` on startup and reading
`client.authStore.collectionName` always gives the collection (e.g. `"users"`). It is derived from
`record.collectionName` when the server includes it, so sessions stored by older versions recover it
too. Superuser sessions have `collectionName === null` by design.

#### Changing the current user's password

```typescript
// Which collection is this session for? (`null` for superuser sessions)
const collection = client.authStore.collectionName;
const userId = client.authStore.model?.id;

if (!collection || !userId) throw new Error('Not signed in with an auth collection');

await client.collection(collection).update(userId, {
  password: 'new-secret',
  passwordConfirm: 'new-secret'
});
```

The record-update rule of the collection must allow the user to update their own record (or the
caller must be a superuser). For the emailed-token flow use
`client.collection(name).requestPasswordReset(email)` and
`confirmPasswordReset(token, password, passwordConfirm)`.

> **Note:** the server does not currently require `oldPassword` for a self-service password change
> (PocketBase does). Until that is enforced server-side, ask for the current password in your UI and
> re-authenticate (`authWithPassword`) before updating if you need that guarantee.

#### Auto token refresh

The SDK automatically refreshes expired auth tokens. When a token expires, the next API call triggers
a transparent refresh via the `auth-refresh` endpoint. No manual intervention needed. This relies on
`authStore.collectionName`, which is why it is persisted with the session.

---

<h2 id="api-reference">API Reference</h2>

### LazypockClient

The main client class.

#### Constructor Options

| Option      | Type           | Default        | Description                                                             |
| ----------- | -------------- | -------------- | ----------------------------------------------------------------------- |
| `baseUrl`   | `string`       | required       | API base URL (e.g. `http://localhost:4000/api`)                         |
| `storage`   | `StorageAdapter` | `memoryStorage` | Custom storage adapter for token persistence                            |
| `authStore` | `AuthStore`    | auto-created   | Explicit auth store instance                                            |
| `realtime`  | `RealtimeService` | auto-created | Real-time service for WebSocket subscriptions                           |

### Collections Service (`client.collections`)

PocketBase-style service for the collections themselves (admin):

- `collections.getList(params?)` — Paginated list of collections
- `collections.getFullList(options?)` — Fetch all collections (auto-paginates)
- `collections.getOne(id, options?)` — Get collection by ID/name
- `collections.create(data, options?)` — Create collection
- `collections.update(id, data, options?)` — Update collection
- `collections.delete(id, options?)` — Delete collection
- `collections.subscribe(cb)` — Subscribe to collection create/update/delete events (returns unsubscribe fn)
- `collections.unsubscribe()` — Unsubscribe from registry events

### CollectionService

Returned by `client.collection(name)`. All reads accept typed query options —
see [Queries](/sdk/typescript/queries) for the full guide.

- `where(field)` — start a **typed filter clause** (see below)
- `select(...fields)` — Project reads to the given fields (see [Queries](/sdk/typescript/queries));
  `select('*')` restores the all-visible default
- `getList(page, perPage, options?)` — Paginated list of records (typed `filter`/`sort`/`expand`/`fields`)
- `getFullList(options?)` — Fetch all records (auto-paginates)
- `getFirstListItem(filter, options?)` — Fetch first record matching filter; `filter` may be a string or a `FilterExpr`
- `getOne(id, options?)` — Get record by ID
- `expandFields(options?)` — List the collection's relation fields (for building `expand`)
- `create(data, options?)` — Create record
- `update(id, data, options?)` — Update record
- `delete(id, options?)` — Delete record
- `subscribe(callback, recordId?)` — Subscribe to record changes (PocketBase-style)
- `unsubscribe(recordId?)` — Unsubscribe
- `typed<T>()` — Cast this service to a record shape (compile-time only)
- `withSchema(schema)` — Bind a schema explicitly (hidden-field exclusion + query checking)
- `authWithPassword(identity, password, options?)` — Login to this auth collection
- `authRefresh(options?)` — Refresh token for this auth collection
- `authMethods(options?)` — Get available auth methods

#### Query options

| Option | Type | Description |
| --- | --- | --- |
| `filter` | `string \| FilterExpr` | PocketBase filter expression, or a builder expression |
| `sort` | `string \| string[]` | Field(s) to sort by; `-field` = descending |
| `expand` | `string \| string[]` | Relation field(s) to expand (`author`, `author.name`, `author.profile`) |
| `fields` | `string` | Explicit field projection for this call (overrides `select()`) |
| `requestKey` | `string \| null` | Override/disable auto-cancellation for this request |
| `singleFlight` | `boolean` | Coalesce concurrent identical requests |
| `fetch` | `typeof fetch` | Custom fetch (tests / React Native) |
| `signal` | `AbortSignal` | Abort signal |
| `headers` | `Record<string, string>` | Extra request headers |

Use the **array** form of `sort`/`expand` to get per-field autocomplete; the
string form works identically but is validated as a whole.

#### Filter builder

`client.collection('posts').where('title')` returns a `FilterBuilder`. Field
names are checked/suggested against the collection and values are escaped.

```typescript
const q = client.collection('posts').where;

q('title').eq('x');                          // title = 'x'
q('title').contains('x');                    // title ~ 'x'
q('views').gte(100);                         // views >= 100
q('id').in(['a', 'b', 'c']);                 // (id = 'a' || id = 'b' || id = 'c')
q('id').notIn(['a', 'b']);                   // (id != 'a' && id != 'b')
q('author').eq(userId);                      // filter by relation id
q('author.email').eq('ada@example.com');     // relation dot-path
q('deleted_at').eq(null);                    // IS NULL
q('title').eq('x').and(q('published').eq(true));
q('a').eq(1).or(q('b').eq(2)).not();
```

| Method | Emits | | Method | Emits |
| --- | --- | --- | --- | --- |
| `eq(v)` | `field = v` | | `anyEq(v)` | `field ?= v` |
| `neq(v)` | `field != v` | | `anyNeq(v)` | `field ?!= v` |
| `contains(v)` | `field ~ v` | | `anyContains(v)` | `field ?~ v` |
| `notContains(v)` | `field !~ v` | | `anyNotContains(v)` | `field ?!~ v` |
| `gt(v)` / `gte(v)` | `field > v` / `field >= v` | | `anyGt(v)` / `anyGte(v)` | `field ?> v` / `field ?>= v` |
| `lt(v)` / `lte(v)` | `field < v` / `field <= v` | | `anyLt(v)` / `anyLte(v)` | `field ?< v` / `field ?<= v` |
| `in(values)` | `(field = a \|\| field = b \|\| …)` | | `and(other)` | `(a && b)` |
| `notIn(values)` | `(field != a && field != b && …)` | | `or(other)` / `not()` | `(a \|\| b)` / `!(a)` |

Values may be `string | number | boolean | null`; the server enforces the
exact per-field type. The raw string form remains available for dynamic or
advanced expressions.

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

### Auto Cancellation

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

#### Per-request control

Pass `requestKey` in the request options to customize the key, or disable auto-cancellation for a
specific request:

```typescript
await client.collection('posts').getList(1, 20, { requestKey: 'my-list' }); // cancelled
await client.collection('posts').getList(1, 20, { requestKey: 'my-list' }); // executed

await client.collection('posts').getList(1, 20, { requestKey: null }); // executed
await client.collection('posts').getList(1, 20, { requestKey: null }); // executed
```

#### Global control

```typescript
// Disable auto-cancellation globally
client.autoCancellation(false);

// Manually cancel pending requests
client.cancelRequest('GET /api/posts?page=1');
client.cancelAllRequests();
```

#### Single-flight dedup (getFullList)

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

### Error Handling

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

### Configuration

#### Storage Adapter

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

#### Auto Token Refresh

The SDK automatically refreshes expired auth tokens. When a token expires, the next API call triggers
a transparent refresh via the `auth-refresh` endpoint. No manual intervention needed.
