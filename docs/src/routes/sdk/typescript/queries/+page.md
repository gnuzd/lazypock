---
title: Queries
---

# Queries

## `select(...)` — pick the fields you want

`select()` projects list/read responses to the given fields (PocketBase `fields` param). Field names
are **type-checked** when the service is typed:

```typescript
const t = await client.collection('posts').select('id', 'title').getList();
// GET /api/posts?fields=id,title

await client.collection('posts').select('id', 'title').getOne('abc123'); // same
```

- `select('*')` (or no `select()` call) — request all visible fields; hidden fields are excluded
  automatically when a schema is available.
- `select()` with no arguments resets back to the default.
- `select()` returns a **derived service** — the original is untouched, so you can keep one default
  service and project per-request.
- Passing an explicit `fields` option overrides the `select()` preset.

When a schema is known (via `types.schemas` or codegen), hidden fields are **not returned by the
server**: every read sends `fields=<visible fields>` by default, and selecting an unknown field logs
a warning.

## `filter` / `sort` / `expand` — type-checked suggestions

With a typed service, the query options validate field names (and filter operators) at compile time —
your editor suggests valid fields as you type:

```typescript
await postsSvc.getList(1, 20, { sort: '-title' }); // ✓ suggests title/published/…
await postsSvc.getList(1, 20, { sort: '-nope' }); // ✗ compile error

await postsSvc.getList(1, 20, {
  filter: "title ~ 'x' && published = true" // ✓ field + operator checked
});
await postsSvc.getList(1, 20, { filter: 'nope = 1' }); // ✗ compile error

await postsSvc.getList(1, 20, { expand: 'author' }); // ✓ field suggested
await postsSvc.getOne('abc', { expand: 'author' });
```

- `filter` — `field op value` clauses with `= != ~ !~ > >= < <=` operators; `&&`, `||`, `!`, and
  parentheses are allowed after the first clause.
- `sort` — `field`, `-field` (desc), `+field`, or comma-separated.
- `expand` — comma-separated relation field names; non-relation fields warn at runtime when a schema
  is available.
- The **untyped** client (`client.collection('posts')` without `typed<T>()`) still accepts any
  string — suggestions kick in once the service is typed.
