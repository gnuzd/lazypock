# lazypock-docs

The documentation site for [lazypock](https://github.com/gnuzd/lazypock) — a
self-hosted backend-as-a-service in Rust + Postgres. Built with SvelteKit and
Tailwind CSS v4, and deployed as a static site.

## Running it

```bash
npm install
npm run dev        # http://localhost:5173
```

```bash
npm run build      # static output
npm run preview
```

## Structure

```
src/
  app.css                    # Tailwind v4 import + the wabisabi-light theme tokens
  app.html                   # HTML shell, sets data-theme="wabisabi-light"
  lib/
    nav.ts                   # sidebar table of contents
    seo.ts                   # site identity, per-page descriptions
    json-ld.ts               # schema.org structured data
    sdk-nav.generated.ts     # GENERATED — do not edit by hand
    components/              # CodeBlock, Seo, ArchDiagram, FlowSteps, ComingSoon
  routes/
    +page.svelte             # landing page ("What is Lazypock")
    architecture/            # how it works: layers, request lifecycle, realtime
    server/                  # Docker, binary, setup, filters, production, env vars
    files/                   # file storage and images
    backup/                  # backup & import JSON format
    sdk/                     # per-SDK pages (generated, see below)
static/
  llms.txt                   # the summary served to AI assistants and crawlers
  backup.schema.json         # machine-readable backup contract
```

## The SDK pages are generated

`src/routes/sdk/<slug>/+page.md` and `src/lib/sdk-nav.generated.ts` are **build
artifacts**. Do not edit them: the next sync overwrites them.

The source of truth for SDK documentation is each SDK's own repository
(`gnuzd/lazypock-ts` → `docs/*.md`). `scripts/sync-sdks.mjs` merges those files
into one scrollable page per SDK — each source file becomes an `<h2>` section —
and regenerates the nav from the rendered sections.

```bash
node scripts/sync-sdks.mjs
```

It resolves an SDK's source in this order:

1. `$LAZYPOCK_TS_DIR` — a local clone, for editing SDK docs and seeing the
   result without pushing the SDK repo first;
2. a cached clone in `.sdk-cache/<slug>`;
3. the already-committed `+page.md`, re-deriving the nav from its sections so the
   site still builds offline.

So to change the TypeScript SDK page: edit `docs/*.md` **in lazypock-ts**, then
run the sync here.

## Theme

The colour tokens are the `wabisabi-light` set, defined once in `src/app.css` as
both the Tailwind v4 `@theme` (which generates utilities like `bg-base-100`,
`text-primary-content`, `rounded-box`) and a `:root` / `[data-theme='wabisabi-light']`
block for runtime switching. To add another theme, copy that block under a new
`[data-theme='…']` selector, change the oklch values, and set `data-theme` on
`<html>` — no Tailwind classes need to change.
