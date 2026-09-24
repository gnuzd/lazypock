---
title: Import / backup JSON format
---

# Import / backup JSON format

Lazypock can export your whole database (collection schemas **and** records) to one JSON file, and
import it back. The same file is used by:

- **Studio** → Settings → **Backups** (download / restore) and Settings → **Import**
- **CLI** → `lazypock backup [file]` / `lazypock restore <file>`
- **HTTP** → `GET /api/export` and `POST /api/import` (superuser only)

The importer is forgiving: it accepts the LazyPock export envelope, a bare array of collections, and
PocketBase 23+ exports. This page describes the canonical format — the one to produce if you are
generating a file by hand or with an AI assistant.

## Machine-readable schema

A [JSON Schema](https://json-schema.org) for the canonical envelope lives at
**<a href="/backup.schema.json">/backup.schema.json</a>**:

```bash
curl -s https://lazypock.gnuzd.dev/backup.schema.json
```

<details>
<summary>Quick shape</summary>

```json
{
  "collections": [
    {
      "name": "posts",
      "type": "base",
      "schema": [{ "name": "title", "type": "text", "required": true }],
      "rules": { "listRule": "", "viewRule": "", "createRule": "", "updateRule": "", "deleteRule": "" },
      "options": { "indexes": [] },
      "records": [{ "id": "...", "title": "Hello" }]
    }
  ]
}
```

</details>

## Envelope

```json
{ "collections": [ /* collection objects */ ] }
```

A bare array (`[ /* collection objects */ ]`) is also accepted. Each collection needs a `name` and
`type`; every **non-view** collection also needs a `schema` (or PocketBase's `fields`) — an empty
list is fine. View collections omit it (their columns come from the view query).

## Collection object

| Key         | Type    | Required | Notes                                                                                     |
| ----------- | ------- | -------- | ----------------------------------------------------------------------------------------- |
| `name`      | string  | yes      | `^[a-z][a-z0-9_]*$` — lowercase table name.                                               |
| `type`      | string  | yes      | `"base"`, `"auth"` or `"view"`.                                                            |
| `schema`    | array   | no       | Field definitions (see below). PocketBase calls this `"fields"` — **use one or the other**. |
| `id`        | string  | no       | Existing collection id. Omit it and the collection is matched/created by `name`.           |
| `rules`     | object  | no       | Access rules (see below).                                                                  |
| `options`   | object  | no       | `{ "indexes": [...] }`, and for views the query (see below).                               |
| `hooks`     | object  | no       | Usually `{}`.                                                                              |
| `records`   | array   | no       | Records to upsert (see below). Omit for `view` collections.                                |

> Auth collections always get a **unique `email`** field — if you declare one, its `unique` flag is
> forced on for you.

## Field object (`schema[]`)

| Key          | Type    | Notes                                                                                            |
| ------------ | ------- | ------------------------------------------------------------------------------------------------ |
| `name`       | string  | `^[A-Za-z][A-Za-z0-9_]*$`. Case is preserved and used verbatim as the API key / DB column.       |
| `type`       | string  | One of the types below.                                                                          |
| `required`   | boolean | `NOT NULL`.                                                                                      |
| `unique`     | boolean | Adds a unique index.                                                                             |
| `indexed`    | boolean | Adds a plain index.                                                                              |
| `hidden`     | boolean | Never returned by the API / hidden in the Studio (e.g. password).                                 |
| `system`     | boolean | **Skip these.** System-managed fields (`id`, `created_at`, `updated_at`, `verified`, …) are stripped on import. |
| `default`    | any     | Default value when the field is omitted on write.                                                |
| `options`    | object  | Type-specific settings (see examples).                                                            |
| `sort_order` | integer | Column order.                                                                                    |

**Field types:** `text`, `editor`, `number`, `bool`, `email`, `url`, `date`, `datetime`, `autodate`,
`select`, `multi_select`, `file`, `multi_file`, `relation`, `json`, `geo`, `password`.

Type options (`options` map) — a few common ones:

```json
{ "name": "status", "type": "select", "options": { "values": ["draft", "published"], "maxSelect": 1 } }
{ "name": "tags", "type": "select", "options": { "values": ["a", "b"], "maxSelect": 3 } }
{ "name": "author", "type": "relation", "options": { "collection": "users", "maxSelect": 1 } }
{ "name": "published_at", "type": "autodate", "options": { "onCreate": false, "onUpdate": true } }
{ "name": "cover", "type": "file", "options": { "maxSelect": 1, "maxSize": 5242880, "mimeTypes": ["image/png", "image/jpeg"] } }
{ "name": "body", "type": "editor" }
{ "name": "meta", "type": "json" }
```

## Rules (`rules`)

PocketBase-style filter expressions, per action. `null` = superusers only, `""` = public.

```json
{
  "listRule": "",
  "viewRule": "",
  "createRule": "@request.auth.id != ''",
  "updateRule": "author = @request.auth.id",
  "deleteRule": "author = @request.auth.id",
  "manageRule": null
}
```

## Indexes and views (`options`)

```json
{ "options": { "indexes": ["UNIQUE email", "created_at DESC, id"] } }
```

View collections carry their query instead of a `schema`:

```json
{ "name": "published_posts", "type": "view", "options": { "view_query": "SELECT id, title FROM posts WHERE status = 'published'" } }
```

## Records (`records[]`)

Records are **upserted by `id`**: existing ids are updated, new ids inserted, so restoring the same
file twice never duplicates rows and relations survive.

- Always give each record an `id` (a UUID). Records without one cannot be matched on re-import.
- Relation fields hold the **target record id** (string), or an array of ids for multi-relations.
- `created_at` / `updated_at` are preserved when present, otherwise stamped by the database.
- **Password is write-only.** Send it as `password` (or the backing `password_hash`); it is bcrypt-hashed
  on write and never returned. Omit it for OAuth-only accounts.
- Never include `system` fields such as `id` in the `schema` list — but records **do** use `id`.

## Minimal example

```json
{
  "collections": [
    {
      "name": "posts",
      "type": "base",
      "schema": [
        { "name": "title", "type": "text", "required": true },
        { "name": "body", "type": "editor" },
        { "name": "status", "type": "select", "options": { "values": ["draft", "published"], "maxSelect": 1 } }
      ],
      "rules": { "listRule": "", "viewRule": "", "createRule": "", "updateRule": "", "deleteRule": "" },
      "options": { "indexes": ["created_at DESC"] },
      "records": [
        { "id": "3f1c0a9e-2b7d-4c1e-9d2a-6f0b1c2d3e4f", "title": "Hello", "status": "published" }
      ]
    }
  ]
}
```

Prefer to leave system fields out entirely? That works too — the server creates `id`, `created_at`
and `updated_at` itself:

```json
{
  "collections": [
    {
      "name": "tags",
      "type": "base",
      "schema": [{ "name": "label", "type": "text", "required": true, "unique": true }],
      "records": [{ "id": "b2f8d0c4-1a2b-4c3d-8e4f-5a6b7c8d9e0f", "label": "release" }]
    }
  ]
}
```

## PocketBase 23+ compatibility

A PocketBase export imports as-is. Differences the importer normalizes for you:

- `"fields"` instead of `"schema"` (do not send both — that's an error).
- camelCase field names and PocketBase collection ids in `relation` options.
- Type options at the **top level** of a field (`"maxSelect": 3`) rather than nested under `"options"`.
- PocketBase's `"system": true` fields (its own `id`, `tokenKey`, …) are stripped.

## Common mistakes

- Sending both `fields` **and** `schema` on the same collection → rejected as ambiguous.
- Including a system `id` field in `schema` → stripped (harmless), but do not rely on it.
- Using `geoPoint`/`multiSelect` style names from the Studio UI — use the canonical `geo` /
  `multi_select` types.
- `deleteMissing` (**destructive**) drops any user collection missing from the file. Leave it `false`
  when importing a partial file.

## For AI assistants

If you are an AI assistant generating a Lazypock import file, use this contract:

1. Output **only** a JSON object, `{ "collections": [ ... ] }`, with no prose or code fences inside the file.
2. Validate against <https://lazypock.gnuzd.dev/backup.schema.json>.
3. Use the canonical field types listed above and nest type settings under `options`.
4. Give every record a stable UUID `id`; references use the target record's id.
5. Omit system fields (`id`, `created_at`, `updated_at`, `verified`, `emailVisibility`, `tokenKey`) from `schema`.
6. Never set `deleteMissing` and never emit SQL.
