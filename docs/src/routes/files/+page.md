# File storage & images

Uploads live in a configured backend — the local disk by default, or an S3-compatible
bucket — and are served back through the API. Images are resized into named **presets**
on first request and the result is cached next to the original.

A file is a row in `_files`, and its **id is what a record's file field holds**. A client
uploads, puts the returned `id` in the field, and reads an image back from
`/api/files/<id>` — it never has to build a storage path itself.

## Where to configure what

| What | Where |
| --- | --- |
| Storage backend, bucket, public base URL | **Settings → Files** (saved in `_settings`, applied immediately) or the environment (`LAZYPOCK_FILES_*`, `LAZYPOCK_S3_*`) as the default |
| Image presets | `PATCH /api/settings` with an `image.presets` list, or **Settings → Media** |
| Who may list and delete files | `PATCH /api/settings` with a `files.rules` map, or **Settings → Media** |
| Per-field type and size limits | the field's options in the collection editor |
| Upload ceiling and reaper interval | environment (`LAZYPOCK_UPLOAD_MAX_BYTES`, `LAZYPOCK_FILE_REAPER_INTERVAL`) |

## Uploading

`POST /api/files` takes a multipart body and requires an authenticated request:

```bash
curl -X POST -H "Authorization: Bearer <token>" \
  -F "file=@cover.png" \
  -F "collection_name=posts" -F "field_name=cover" -F "record_id=<record id>" \
  https://your-host/api/files
```

Parts:

| Part | Required | Meaning |
| --- | --- | --- |
| `file` | yes | The bytes. The filename on the part is what gets stored. |
| `collection_name` (or `collection`) | no | The collection the file belongs to. It has to exist. |
| `record_id` (or `record`) | no | The record it belongs to. It has to exist, unless it does not exist *yet* — a form uploads before the record is saved. |
| `field_name` | no | The field it is for. |
| `origin` | no | `field` (default), `editor` or `library`. See **Abandoned uploads**. |
| `variants` | no | Presets to generate before responding, e.g. `content,thumb`. |

**An upload with no record is normal.** A record form uploads the file first and posts the
id with the rest of the form, and the media library and the rich-text editor upload
without a record at all. Such a file is stored `pending`, under a `library/<id>/<name>`
key, and appears in the media library immediately. A record write that names it attaches
it (`status: ready`, `attached_at` set).

**A file belongs to whoever uploaded it.** Uploading only requires an authenticated
request; the authority to put a file *into* a record is the record write's own rules,
which is where the attach happens. Two consequences worth knowing before writing a form:

- only the uploader (or a superuser) can attach it — a record write by another account
  that names somebody else's file id is refused with `403`, so a file id read off a
  public record cannot be claimed by a stranger;
- a file that already belongs to one record cannot be moved to another — that write is
  refused with `409`. Re-saving the same file field on the same record is fine.

An upload that names a record does not check that the caller may *write* that record, so
a stray upload can still land in the library with another collection's name on it. That is
a quota matter rather than a data one: it cannot attach itself, and every upload is
bounded by `LAZYPOCK_UPLOAD_MAX_BYTES`.

**Images only.** The four formats this server can read: **png, jpeg, gif, webp**. The
type is decided by the bytes, never by the filename or the declared `Content-Type` — an
SVG renamed to `.png` is refused, because an SVG served as an image can execute script.
The default ceiling is **5 MiB** per file (`LAZYPOCK_UPLOAD_MAX_BYTES`), and the upload
route's own body limit is 16 MiB.

The response is the file row:

```json
{
  "id": "Abc123xyz789abc",
  "filename": "Abc123xyz789abc_cover.png",
  "size": 20480,
  "mime_type": "image/png",
  "width": 800,
  "height": 600,
  "checksum": "…",
  "url": "/api/files/Abc123xyz789abc",
  "key": "posts/<record id>/Abc123xyz789abc_cover.png",
  "status": "ready",
  "origin": "field",
  "variants": {
    "content": "/api/files/Abc123xyz789abc",
    "thumb": "/api/files/Abc123xyz789abc?preset=thumb"
  }
}
```

Put **`id`** in the file field. `url` is what to render; `key` is where the object actually
lives, which a client only needs if it addresses storage directly.

## Serving

A file is fetched by its id:

```text
GET /api/files/<id>                        the original
GET /api/files/<id>?preset=<name>          a preset
GET /api/files/<id>?thumb=400x300          an ad-hoc box
GET /api/files/<id>/thumbs/<size>          the same, as a path
GET /api/files/<id>/scale/<size>           the same, as a path
```

The older key-addressed form works too:
`GET /api/files/<collection>/<record>/<filename>`.

Serving is public and cached (`Cache-Control: immutable`): the key contains a random
component, so a URL behaves as a capability. That is the same model PocketBase uses, and
it is why the access rules below cover listing and deleting rather than reading.

Sizes are generated on the first request and stored next to the original, so a preset
costs a resize once. AVIF output is refused loudly: the presets table allows it, the
encoder cannot write it.

## Image presets

A preset is a name, a box, a fit mode (`cover`, `contain`, `fill`, `inside`, `outside`), an
output format and a quality. `sm`, `md` and `lg` fit *inside* the box and never enlarge, so
a small source keeps its own resolution; `thumb` is a square cover crop for grid tiles.

Four are seeded (`thumb` 200×200, `sm` 320, `md` 768, `lg` 1600). They are rows in
`_image_presets`, editable with `PATCH /api/settings` or in **Settings → Media**, and
`GET /api/settings` returns them so a client can discover the names it may ask for.

Resizing happens **in this process** — there is no ImageMagick and no sidecar to install.
The trade-off is that the API's own memory and CPU do the decoding, so the size of an
image a client may upload is bounded by `LAZYPOCK_UPLOAD_MAX_BYTES` and the presets' own
box limit (8192 px).

## Direct upload (skipping the multipart body)

A client that would rather send the bytes straight to the bucket does it in three calls:

1. `POST /api/files/presign` with `{ "filename", "size", "mime", "collection_name", "field_name", "record_id" }`.
   The row is reserved and the response is `{ "id", "key", "method": "PUT", "url", "headers" }`.
2. `PUT` the file to `url` with `headers` (the bucket's own URL, not this server's).
3. `POST /api/files/<id>/complete` — the server verifies what arrived and answers with the
   file row, exactly as a multipart upload would.

What the client *claims* is checked at step 1 (the size, the declared type, the collection
and the record) and what actually arrived is checked at step 3: a presigned URL authorises
a PUT to one key, never a content type or a size, so the bytes are sniffed and measured
again before the row is finalised. The URL is valid for **15 minutes**. An id that never
received an object is dropped rather than left as a file that cannot be served.

**A local backend cannot presign** — a directory has no HTTP endpoint — so step 1 answers
`501` with the reason and the reservation is dropped. The SDK reads that as "no direct
upload here" and posts the bytes to `POST /api/files` instead, so a client can always call
`uploadDirect` and let the server decide.

## The media library

`GET /api/files` lists what is stored, newest first, **including files that belong to no
record yet**. Filters: `q` (filename contains), `mime` (`image/` for images only),
`collectionName`, `fieldName`, `page`, `perPage`. The envelope carries `items`, `total`,
`totalItems`, `page`, `perPage` and `totalPages`.

Each item is the same shape an upload returns, so a picker renders a fresh upload and an
old file with one code path.

`DELETE /api/files/<id>` removes the row, which queues the object and its variants for
removal (see below). Two guards:

- **A file a record still references is refused** with `409` and the list of references —
  deleting it would leave that record pointing at nothing. A superuser can insist with
  `?force=true`.
- The file rules below decide who may delete at all.

## File rules

`files.rules` in settings holds the two rules, exactly as a collection's rules work:

| Value | Meaning |
| --- | --- |
| absent / `null` | Superusers only |
| `""` | Public |
| a filter | Conditional, evaluated per file |

```json
{ "files": { "rules": {
  "listRule": "uploaded_by = @request.auth.id || @request.auth.role = 'admin'",
  "deleteRule": "uploaded_by = @request.auth.id || @request.auth.role = 'admin'"
} } }
```

`@request.auth.id`, `@request.auth.email` and any custom field of the authenticated
record — `@request.auth.role` in this example — are available on **either** side of a
comparison. The record's columns are read on each authenticated request and published to
the database session, so `@request.auth.role = 'admin'` is true for an account whose
`role` is `admin`; a rule that references a field the record does not have resolves to
nothing and therefore matches no row. The columns available on the rule's own side are
`_files`' own: `filename`, `mime_type`, `extension`, `size`, `origin`, `status`,
`collection_name`, `field_name`, `record_id`, `uploaded_by`, `width`, `height`,
`created_at`, and the rest of the row.

A superuser bypasses both rules, which is what keeps the media library usable when the API
has been locked down. A rule that cannot be read — whitespace only, a number, a broken
filter — **denies** rather than quietly meaning "public", and says so in the log.

The rules are compiled to Postgres row-level security policies on `_files` and rebuilt
whenever `files.rules` changes, so a query that forgets to filter is denied by the
database rather than returning every row. Serving is still not a rule check: a file URL
is a capability, as described above.

Nothing configured means superusers only: a fresh installation behaves as an admin tool
until an operator opens it up.

## Storage backends

Local (the default) writes under `LAZYPOCK_FILES_DIR` (`data/files`):

```bash
LAZYPOCK_FILES_ADAPTER=local
LAZYPOCK_FILES_DIR=/var/lib/lazypock/files
```

S3-compatible, Cloudflare R2 included:

```bash
LAZYPOCK_FILES_ADAPTER=s3
LAZYPOCK_S3_ENDPOINT=https://{account_id}.r2.cloudflarestorage.com
LAZYPOCK_S3_BUCKET=my-bucket
LAZYPOCK_S3_ACCESS_KEY=…
LAZYPOCK_S3_SECRET_KEY=…
LAZYPOCK_S3_REGION=auto
LAZYPOCK_FILES_PUBLIC_URL=https://cdn.example.com   # optional: serve objects from your CDN
```

An incomplete S3 configuration is refused at boot rather than on the first upload, and the
backend is health-checked. With a public base URL the URLs a client is handed point at the
CDN; without one they point at this server.

**Settings → Files** shows this configuration (`GET /api/settings/storage`, with the secret
reported only as "set"), **saves it** (`PATCH`, superuser-only) and probes the backend
(`POST /api/settings/storage/test`).

A save takes effect **immediately** — the running file service is rebuilt and swapped in, so
there is no restart in the middle — and it is kept in `_settings`, so it survives one. The
environment supplies the defaults and a saved field wins over it; a field the environment
set and nothing has overridden is shown locked, with the variable that owns it.

Two things to know before you save a secret:

- It is stored **encrypted**, with a key only the environment holds:
  `LAZYPOCK_SECRET_KEY=$(openssl rand -hex 32)`. Without that variable a save that includes a
  secret is refused rather than stored in the clear — `_settings` travels in every backup,
  replica and `pg_dump`. Losing the key means the saved secret cannot be read again and has
  to be entered once more.
- Two form fields are refused rather than stored and ignored: **Force path style** has to stay
  on (an object is addressed as `<endpoint>/<bucket>/<key>`) and **Key prefix** must be empty
  (an object's key is `{collection}/{record}/{filename}`).

`LAZYPOCK_PRESIGN_TTL` (or the page's **Presign TTL**) sets how long a presigned URL stays
valid; the default is fifteen minutes.

### Deletion and the reaper

Deleting a file — through the API, by deleting the record that owns it, or by dropping the
whole collection — removes the row and writes the object into a deletion queue **in the
same transaction**, through a database trigger. The object is removed afterwards by a
worker, so a crash in between cannot leave a row pointing at an object that is gone, and a
storage outage only delays the cleanup.

The worker runs every hour by default (`LAZYPOCK_FILE_REAPER_INTERVAL` in seconds, `0`
disables it and says so loudly). It also reclaims **abandoned uploads**: an editor or
library upload nobody ever attached is deleted once it is older than
`files.rules.unattached_ttl_ms` (default one day). An upload that arrived through a
record's field is never reclaimed, even while it is pending — the user meant that file to
be in a record, and a failure to attach it is not a reason to delete their upload.

## API reference

| Method | Path | Who | What |
| --- | --- | --- | --- |
| `POST` | `/api/files` | authenticated | Multipart upload |
| `POST` | `/api/files/presign` | authenticated | Reserve an object and get a presigned `PUT` (S3) |
| `POST` | `/api/files/{id}/complete` | authenticated | Verify a direct upload and finish the row |
| `GET` | `/api/files` | per `listRule` | List the library, with filters and paging |
| `GET` | `/api/files/{id}` | public | The file, or one of its variants |
| `DELETE` | `/api/files/{id}` | per `deleteRule` | Delete a file (409 while referenced) |
| `GET` | `/api/files/{collection}/{record}/{filename}` | public | The file by its storage key |
| `DELETE` | `/api/files/{collection}/{record}/{filename}` | per `deleteRule` | The same delete, addressed by key |

## Troubleshooting

| Symptom | Cause |
| --- | --- |
| `400` "the file is not a supported image" | Not a png/jpeg/gif/webp, or an SVG renamed — the bytes decide |
| `400` "a `collection` part is required" | The multipart body has no `collection_name`/`collection` part |
| `403` on `GET /api/files` | No `file listRule` is set, or it does not match. A superuser token always works |
| `409` on delete | A record still references the file; `?force=true` as a superuser breaks the reference |
| `501` on `/api/files/presign` | The backend is local; post the bytes to `/api/files` instead |
| `404` on a file that was just uploaded | The row is gone (the upload was cancelled or the file was deleted) — or the id is not the file's |
| No `variants` in the response | The file is not an image, or no presets are configured |
| An upload disappeared after a day | It was an unattached `editor`/`library` upload; see `files.rules.unattached_ttl_ms` |
| Thumbnails are slow the first time | They are generated on first request and cached afterwards |
