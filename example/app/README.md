# LazyPock Example Apps

Frontend examples that consume the LazyPock server from
[`../docker/`](../docker/) (or any LazyPock instance).

## react — blog demo (React + Vite)

A small blog app (auth, posts, comments) that uses the **typed SDK client** —
`schema/` collections are codegen'd into `src/lib/lazypock.types.ts`, so
`client.collection("posts")` is fully typed and collection names are suggested
by the IDE.

```bash
cd react
npm install

# Regenerate the typed client from your server (optional — a snapshot is committed):
#   npx lazypock --url http://localhost:4000/api --email admin@lazypock.app --password admin123 --out src/lib/lazypock.types.ts

npm run dev          # → http://localhost:5173
```

It expects the LazyPock server at `http://localhost:4000/api` (see
[`../docker/`](../docker/) for `docker compose up`) with a `posts` collection
and an auth `users` collection — the bundled seed creates `posts` on first
boot, and the `docker/hooks/post_hooks.ex` example auto-generates a `slug`
on post create.

> The committed `lazypock.types.ts` requires the SDK's `SchemaField.collectionId`
> fix (lazypock-ts ≥ 0.5.1). Regenerate it with `npx lazypock` once that lands.

## More frameworks — coming

- `svelte/` — SvelteKit
- `vue/` — Vue + Vite
- `next/` — Next.js

Every app uses the published npm SDK: `npm install lazypock`.
See the SDK repo — [github.com/gnuzd/lazypock-ts](https://github.com/gnuzd/lazypock-ts) —
for the full API.
