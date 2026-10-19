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

> **Large databases use an archive instead.** The single JSON document has to be built in memory, so
> it cannot hold a multi-GB database (or a single record over 100 MB). For those, use the **NDJSON
> archive** described below. The JSON format on this page is unchanged and still fully supported.

## NDJSON archive (large databases)

For anything that does not comfortably fit in one JSON document, Lazypock can stream a **zip archive**
instead:

```text
backup-<timestamp>.zip
├── manifest.json          # format + version + per-collection row counts + file totals
├── schema.json            # { "collections": [...] } — collection definitions, no records
├── data/<collection>.ndjson
├── files.ndjson           # one _files row per line (upload metadata)
└── files/<storage_path>   # the uploaded blobs themselves
```

Each `data/*.ndjson` file holds **one JSON object per line, one line per record**, so:

- the exporter never holds more than one record in memory at a time — a single 100 MB+ `editor` value
  is just one long line;
- it is easy to preview or grep a collection without loading the rest (`grep` a collection file,
  `tail` the last records, count rows with `wc -l`).

**Uploaded files are included.** `files.ndjson` carries the `_files` metadata rows (so the record →
file links survive) and `files/` carries the blobs at their original `storage_path`. Blobs are copied
through the configured storage adapter: a local backend is copied on disk rather than buffered into
memory, and the restore writes each blob back to the exact path its `_files` row references. A
metadata row whose blob was already missing when the backup was taken is counted in
`manifest.files.missing` rather than aborting the backup. Pass `--no-files` on the CLI (or
`include_files: false` in `Backup.export_stream/1`) for a schema+records-only backup.

> Blob backup uses the configured storage adapter, so it works for **both** the local and the S3/R2
> backend: blobs are read with the adapter's `local_path/1` when it has one (a local copy on disk) and
> fall back to `get/1` otherwise, and restore writes them back at the exact `storage_path`. An S3/R2
> deployment therefore backs up its objects like any other file. A metadata row whose blob was already
> missing when the backup was taken is counted in `manifest.files.missing` rather than aborting.

See [File storage & images](/files) for the storage backends, presets and the media library.

### Exporting it

```bash
# HTTP (superuser): streamed as a download with a Content-Length
curl -H "Authorization: Bearer <token>" -o backup.zip https://your-host/api/export/archive

# CLI: writes an archive by default (pass a .json path for the legacy format)
lazypock backup backup.zip
lazypock backup legacy.json

# See what an archive contains without restoring it (no database needed)
lazypock inspect backup.zip
```

In the Studio, **Settings → Backups → Download Backup (.zip)** uses this path and shows real download
progress. The **Download JSON** button next to it keeps producing the single-document format above.

### Importing it

In the Studio, selecting an archive on **Settings → Import** or **Settings → Backups** lists what it
contains *before* anything is uploaded: both read `manifest.json` out of the archive in the browser
(only the first few KB of the file are read), so previewing a multi-GB archive costs nothing.

`POST /api/import` accepts the archive as a `multipart/form-data` upload (field name `file`). The
request body limit for this route alone is raised (`LAZYPOCK_IMPORT_MAX_MB`, default 10240 MB), so a
multi-GB upload is not rejected with a `413`:

```bash
curl -X POST -H "Authorization: Bearer <token>" \
  -F "file=@backup.zip" -F "password=<your superuser password>" \
  https://your-host/api/import
```

`lazypock restore backup.zip` does the same thing from a shell, and the Studio's **Backups → Restore**
accepts either a `.zip` archive or the JSON file.

Field-name semantics are identical to the JSON format: names are kept **verbatim** (`tagColor` stays
`tagColor`), and records are upserted by `id`.

An import is a **declaration**, not a patch. A collection the payload names is created, or updated to
match it — its fields, rules and options — and the records it carries are upserted by `id`. A record
or collection the payload leaves out is left exactly as it is, unless you ask for the difference to
be applied: **`deleteMissing`** drops the user collections *and* fields the payload does not declare,
which is what the Studio's **Delete missing collections and schema fields** switch sends. It is off by
default, in the Studio and on the API, because it is the one import option that destroys data: a
column it drops takes its contents with it.

Two things are never dropped, whatever the payload or the option says: a system collection (any
`_`-prefixed table) and a built-in column — `id`, `created`, `updated`, and on an auth collection
`email`, `password_hash`, `verified`, `emailVisibility` and `token_key`. A payload may name them (a
PocketBase export names its own); naming them keeps them, and a field marked `"system": true` is
stripped from the fields an import creates or alters rather than declared.

### Undo, and what happens on large databases

Before an import, Lazypock writes an automatic undo checkpoint (itself an NDJSON archive) so the last
import can be rolled back with **Undo last import**. That is only done while the database is below a
size threshold, because checkpointing a very large database costs time and disk proportional to its
size.

Above the threshold the checkpoint is skipped, and instead of silently losing rollback the import
stops and asks for explicit confirmation (a dialog in the Studio, a `--no-undo-checkpoint` flag or
interactive prompt on the CLI, a `409` with `requires_confirmation: true` from HTTP).

The checkpoint holds collection definitions as well as rows, which is what lets the rollback undo the
destructive half of an import too: a collection `deleteMissing` dropped is recreated, and a field it
pruned comes back with its column *and* its values, because the rows are restored from the snapshot
after the column is.

| Variable | Default | Meaning |
| --- | --- | --- |
| `LAZYPOCK_IMPORT_UNDO_MAX_MB` | `1024` | Database size above which the automatic undo checkpoint is skipped and confirmation is required (`0` always requires confirmation). |
| `LAZYPOCK_IMPORT_UNDO_KEEP` | `5` | How many undo checkpoints to keep. |
| `LAZYPOCK_BACKUP_DIR` | `<priv>/backups` | Where backups and checkpoints are written. |
| `LAZYPOCK_IMPORT_MAX_MB` | `10240` | Request body limit for `POST /api/import` only. |
| `LAZYPOCK_IMPORT_BATCH_SIZE` | `500` | Records per batched upsert while importing. |
| `LAZYPOCK_IMPORT_BIG_ROW_MB` | `5` | A record larger than this is inserted on its own instead of batched. |

If the database looks Neon-hosted (`*.neon.tech`), the same confirmation message also points at Neon
branching / point-in-time restore as an alternative safety net. Set `LAZYPOCK_NEON_NOTICE=0` to silence
that note. It is informational only — Lazypock makes no Neon API calls.

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
`type`; a **base** or **auth** collection also needs a `schema` (or PocketBase's `fields`) — an empty
list is fine. A **view** collection needs its query instead (`viewQuery`, or `viewBuilder`), and
carries no records.

## Collection object

| Key           | Type   | Required | Notes                                                                                                                      |
| ------------- | ------ | -------- | -------------------------------------------------------------------------------------------------------------------------- |
| `name`        | string | yes      | `^[a-z][a-z0-9_]*$` — lowercase table name.                                                                                |
| `type`        | string | yes      | `"base"`, `"auth"` or `"view"`.                                                                                            |
| `schema`      | array  | no       | Field definitions (see below). PocketBase calls this `"fields"` — send either; if a collection has both, `fields` is used. A view's columns come from its query instead. |
| `id`          | string | no       | Existing collection id. Omit it and the collection is matched/created by `name`.                                           |
| `rules`       | object | no       | Access rules (see below).                                                                                                  |
| `options`     | object | no       | `{ "indexes": [...] }` for a table; `{ "view_query": "SELECT …" }` or `{ "view_builder": {…} }` for a view (see below).    |
| `viewQuery`   | string | no       | PocketBase's spelling of a view's SQL; folded into `options.view_query`. Also accepted as `viewBuilder` for the no-code spec. |
| `hooks`       | object | no       | Usually `{}`.                                                                                                              |
| `records`     | array  | no       | Records to upsert (see below). A view carries none — its rows are its query's.                                             |

> Auth collections always get a **unique `email`** field — a payload that declares one has it treated
> as the built-in column, so declaring it is harmless but achieves nothing.

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
| `default`    | any     | A real column default: the value written when the field is omitted. See the note below.          |
| `options`    | object  | Type-specific settings (see examples).                                                            |
| `sort_order` | integer | Column order. The order of `schema` is used when it is absent.                                    |

**Field types:** `text`, `editor`, `number`, `bool`, `email`, `url`, `date`, `autodate`, `select`,
`file`, `relation`, `json`, `geo`, `password`.

The multi-value spellings are accepted as aliases of the types that carry them, because a multi
select *is* a `select` with `maxSelect > 1` and a multi file *is* a `file`:

| Alias           | Stored as  | Meaning                                    |
| --------------- | ---------- | ------------------------------------------ |
| `datetime`      | `date`     | A timestamp (`timestamptz`).               |
| `multi_select`  | `select`   | `select` with `maxSelect > 1`.             |
| `multi_file`    | `file`     | `file` with `maxSelect > 1`.                |
| `geo`/`geoPoint`| `geoPoint` | `{ "lon": …, "lat": … }`.                   |

`default` is applied to the column itself, so both a write that omits the field and plain SQL get it.
It must match the field's type: a string for text-like and select fields (a select default must be one
of its `values`), a number for `number`, `true`/`false` for `bool`, a date string for `date`, and any
JSON value for `json`/`geo`. A `multi_select`, `multi_file` or `relation` field cannot declare one —
those are refused rather than silently ignored.

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

`maxSize` is enforced on uploads; `maxFileSize` is accepted as the equivalent older spelling.

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

`@request.auth.*` may be on either side of a comparison — both `@request.auth.role = 'admin'` and
`author = @request.auth.id` are valid. `@request.auth.role` reads the `role` column of the
authenticated record (loaded once per authenticated request), so the rule follows the record rather
than living in the token; a rule that names a column the record does not have matches nothing. A rule
that compares two *fields* (`a = b`) is refused.

## Indexes (`options.indexes`)

```json
{ "options": { "indexes": ["UNIQUE email", "created_at DESC, id"] } }
```

Each entry is either an expression list — `[UNIQUE ]column [ASC|DESC][, …]` — or a full PocketBase
statement, which is parsed and rebuilt rather than executed as given:

```json
{ "options": { "indexes": ["CREATE UNIQUE INDEX idx_posts_slug ON posts (slug) WHERE published = true"] } }
```

- Only the collection's **own columns** can be indexed. A generated name is `idx_<collection>_<columns>`;
  an explicit statement keeps its name.
- A `WHERE` predicate is a rule filter, compiled exactly like an access rule.
- **Refused, with a message:** index *expressions* (`lower(email)`), `USING`, `COLLATE`, `INCLUDE`,
  a statement that names a different collection, and an unknown column. Nothing is silently dropped.
- A generated name that would collide with a name the collection already holds (a `unique` field's
  index, the auth email index) gets a deterministic suffix, so re-importing the same file is a no-op.
- An **explicit** name that already exists on a collection but was not declared before is left as it is
  (replacing the case-insensitive unique email index would weaken it); the import logs a warning rather
  than failing. Rename the declared index to apply it alongside.

Dropping an entry from `options.indexes` on a later import drops the index.

## Views (`type: "view"`)

A view is a read-only collection over a Postgres `VIEW`. Give it a query, and its columns are the
query's:

```json
{ "name": "published_posts", "type": "view",
  "options": { "view_query": "SELECT id, title, created FROM posts WHERE status = 'published'" },
  "rules": { "listRule": "" } }
```

The no-code builder is accepted too, and its generated SQL is what is stored (so editing the SQL and
the builder are the same thing downstream):

```json
{ "name": "titles", "type": "view",
  "options": { "view_builder": { "source": "posts", "fields": [{ "source": "posts", "name": "title" }] } } }
```

- The query must be **one SELECT** and must expose an `id` column — the records API addresses records
  by id. A `;` is refused, not executed.
- Views are read-only: `POST`/`PATCH`/`DELETE` on its records answer 400, and the batch endpoint
  refuses a view.
- Views are created `WITH (security_invoker = true)`, so the source tables' policies still apply, and
  the view's own `rules` are enforced by the query layer (a view cannot carry Postgres policies). An
  absent rule is superuser-only, `""` is public, as on a table.
- No indexes (a view uses its sources'), no realtime events, and keyset paging (`after=`) needs a
  `created` column — a view without one uses offset paging.

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

A PocketBase export imports as-is, apart from the few differences the importer normalizes or refuses
with a message:

- `"fields"` instead of `"schema"`. A collection carrying **both** is fine too: `fields` is used.
  The Studio's own "Export Collections" file looks like that, because it posts back the collection
  objects the API serves.
- camelCase field names and PocketBase collection ids in `relation` options.
- Type options at the **top level** of a field (`"maxSelect": 3`) rather than nested under `"options"`.
- PocketBase's `"system": true` fields (its own `id`, `tokenKey`, …) are stripped.
- PocketBase index statements in `options.indexes` (see above). One that indexes an *expression* is
  refused with a message rather than silently skipped, because it cannot be rebuilt safely.
- A PocketBase `"type": "view"` collection: its top-level `viewQuery` is folded into
  `options.view_query`. The view is re-created with `security_invoker`, and its records are not
  imported (a view has none of its own).

## Common mistakes

- Sending both `fields` **and** `schema` on the same collection → `fields` wins, `schema` is ignored
  (they mean the same thing; the derived `schema` beside `fields` is the poorer of the two).
- Including a system `id` field in `schema` → stripped (harmless), but do not rely on it.
- Using a field type that is not listed above, or a `geoPoint`/`multiSelect`/`multiSelect` style name from
  a PocketBase export — the aliases in the table above cover the PocketBase spellings (`geo`,
  `multi_select`, `multi_file`, `datetime`); anything else is refused by name.
- Declaring an index on another collection's column, or an index expression — the index list only covers
  the collection's own columns (see above).
- `deleteMissing` (**destructive**) drops every user collection **and field** the file does not
  declare. Leave it `false` when importing a partial file. System collections and built-in columns are
  never dropped.

## For AI assistants

If you are an AI assistant generating a Lazypock import file, use this contract:

1. Output **only** a JSON object, `{ "collections": [ ... ] }`, with no prose or code fences inside the file.
2. Validate against <https://lazypock.gnuzd.dev/backup.schema.json>.
3. Use the canonical field types listed above and nest type settings under `options`. `default` is a
   top-level key on the field, not an option.
4. Give every record a stable UUID `id`; references use the target record's id.
5. Omit system fields (`id`, `created_at`, `updated_at`, `verified`, `emailVisibility`, `tokenKey`) from `schema`.
6. Write indexes in `options.indexes` as expressions (`"UNIQUE email"`, `"created_at DESC, id"`), never
   as SQL to be executed.
7. Never set `deleteMissing` and never emit SQL.
