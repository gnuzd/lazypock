---
title: File storage & images
---

# File storage & images

Lazypock stores uploads either on the server's disk (**local**, the default) or in an
**S3-compatible bucket** (AWS S3, Cloudflare R2, MinIO). Images are validated on the way in, resized
into named **presets**, cached, and served either from the app or from your CDN.

Everything below is available **today** in the Studio, the REST API, the CLI and the TypeScript SDK.

## Where to configure what

| I want to… | Go to |
| --- | --- |
| Turn on S3/R2 and test the connection | Studio → **Settings → Files Storage** |
| Change the global upload size / allowed types / image caps | `PATCH /api/settings` with an `upload` key (or the same page) |
| Limit one field differently | the field's options in the collection editor (`maxFileSize`, `mimeTypes`, `thumbs`) |
| Add or change an image preset | `PATCH /api/settings` with an `image.presets` list |
| Browse or delete uploaded files | Studio → **Media** |
| Insert an image into a richtext field | the 🖼 button in the editor toolbar |

---

## Uploading

`POST /api/files` (`multipart/form-data`, field name `file`) is what the SDK, the Studio and the
richtext editor all use. The server validates in this order and stops at the first failure:

1. **Body size** — the request-body cap for this route is `LAZYPOCK_UPLOAD_MAX_MB` (default 20 MB).
2. **File size** — the effective policy limit (default 10 MB, see below) → `413`.
3. **Extension + magic bytes** — the file's real contents must match an allowlisted extension
   (a PHP script renamed `.png` is rejected) → `400`.
4. **MIME allowlist** — if the field/settings restrict types → `400`.
5. **Image dimensions** — megapixel and per-side caps → `422`.
6. **Store + generate variants** → `201` with the file record.

| Response | Meaning |
| --- | --- |
| `201` | Stored; the body is the file record (see *API reference*) |
| `400` | Wrong type / contents do not match the extension / disallowed MIME |
| `413` | Too large (request body or file) |
| `415` | Not an accepted type (direct-upload verification) |
| `422` | Image over the megapixel or dimension cap; direct-upload size mismatch |
| `503` | The image queue is saturated — retry after the `Retry-After` header |

### Upload policy

Limits resolve **field option → global settings → built-in default**:

| Limit | Field option | Global setting | Default |
| --- | --- | --- | --- |
| Max size | `maxFileSize` | `upload.max_size` | 10 MB |
| Allowed types | `mimeTypes` | `upload.mime_types` (optional; `image/*` wildcards allowed) | the built-in allowlist |
| Megapixels | — | `upload.max_megapixels` | 40 |
| Max side | — | `upload.max_dimension` | 10000 px |

```json
// PATCH /api/settings
{ "upload": { "max_size": "5MB", "mime_types": ["image/*"], "max_megapixels": 40 } }
```

`LAZYPOCK_UPLOAD_MAX_MB` overrides the size setting. Leave a field's option blank to inherit the
global value — the collection editor shows where each default comes from.

---

## Images: presets & variants

A **preset** is a named resize. Defaults ship `thumb` (100×100 cover) and `content` (1280 px wide,
contain) — both generated **eagerly on upload**, so the URLs in the response work immediately.

```json
// PATCH /api/settings
{ "image": { "presets": [
  { "name": "thumb",   "width": 100,  "height": 100, "fit": "cover",   "quality": 80, "eager": true },
  { "name": "content", "width": 1280, "height": null, "fit": "contain", "quality": 80, "eager": true },
  { "name": "card",    "width": 640,  "height": null, "fit": "contain", "quality": 75, "eager": false }
] } }
```

| Key | Meaning |
| --- | --- |
| `name` | Used in the URL: `GET /api/files/:id/scale/:preset` |
| `width` / `height` | Target box in pixels (`height: null` = keep aspect) |
| `fit` | `cover` (fill and centre-crop), `contain` (fit inside, never upscale), `exact` (stretch) |
| `quality` | WebP quality, 1–100 |
| `eager` | Generate during upload instead of on first request |

Preset variants are always **WebP**, metadata is stripped and orientation applied. Requests for the
same missing variant are coalesced, so a burst of traffic generates it once.

Also available:

- `GET /api/files/:id/scale/300x200` — arbitrary size, capped at **2000 px per side** (rejected with `400` otherwise).
- `GET /api/files/:id/thumbs/:size` — legacy thumbnail route (kept for compatibility).
- Upload accepts `?variants=content,thumb` to force specific presets before the response.

### Image engine & requirements

Every resize shells out to the **ImageMagick CLI**. That is a **host dependency**: a prebuilt
single-binary release does **not** bundle it, so install it on the machine (or in the container
image) that runs the app — see the [Server Guide](/server#images) for the install/verify steps.

| Command | Used for |
| --- | --- |
| `identify` | Reading image dimensions (the megapixel/dimension caps, and direct uploads) |
| `magick` / `convert` | The actual resize (thumbnails, presets, `/scale/:size`) |

**ImageMagick 6** (what Debian/Ubuntu install: `identify` + `convert`) and **ImageMagick 7**
(`magick`) are both supported:

```bash
sudo apt install imagemagick      # Debian / Ubuntu (IM6)
brew install imagemagick          # macOS (IM7)
identify -version                 # verify
```

If ImageMagick is not installed, uploads still work and originals are served unchanged, but no
variants are produced:

- `GET /api/files/:id/thumbs/:size` → `404`
- `GET /api/files/:id/scale/:preset` → `400`
- a one-time warning is logged on the first upload; set `LAZYPOCK_THUMBNAILS=0` to disable resizing
deliberately

Image work always runs through a per-instance **limiter**, so a burst of uploads cannot exhaust the
server:

| Variable | Default | Purpose |
| --- | --- | --- |
| `LAZYPOCK_IMAGE_CONCURRENCY` | `1` | Concurrent image jobs per instance (overload → `503`) |
| `LAZYPOCK_MAGICK_MEMORY_LIMIT` | `256MiB` | Per-process ImageMagick memory cap |
| `LAZYPOCK_THUMBNAILS` | — | Set to `0` to disable all resizing |
| `LAZYPOCK_IMAGE_ENGINE` | `magick` | Image engine (selecting an unavailable engine fails loudly) |
| `LAZYPOCK_VARIANT_CACHE_MAX` | `5GB` | Local variant cache cap (oldest evicted first) |

---

## Storage backends

### Local (default)

Files live under the app's `priv/uploads/YYYY/MM/DD/`, variants under `priv/uploads/_variants/:file_id/`,
and the ad-hoc `/scale/:size` cache under `priv/uploads/_cache/scale/`. Zero configuration.

### S3 / R2

Open **Settings → Files Storage**, fill in the endpoint, bucket and keys, then press **Test
connection** — it performs a real `PUT` → `HEAD` → `GET` → `DELETE` round-trip and reports each step
(credentials, endpoint, permissions, CORS), so you see exactly what failed.

| Field | Cloudflare R2 |
| --- | --- |
| Endpoint | `https://{account_id}.r2.cloudflarestorage.com` |
| Region | `auto` |
| Bucket | your bucket name |
| Key prefix | e.g. `lazypock/my-app/` (objects live under `{prefix}{file_id}/`) |
| Public base URL | your CDN domain (optional) |
| Force path style | **on** (safest for R2/MinIO) |
| Presign TTL | `900` seconds |

Prefer environment variables in production — they **win** over the Studio and the matching fields
are locked in the UI:

```bash
LAZYPOCK_S3_ENDPOINT=https://{account_id}.r2.cloudflarestorage.com
LAZYPOCK_S3_BUCKET=my-bucket
LAZYPOCK_S3_ACCESS_KEY=...
LAZYPOCK_S3_SECRET=...
LAZYPOCK_S3_REGION=auto
LAZYPOCK_S3_PREFIX=lazypock/my-app/
LAZYPOCK_S3_PUBLIC_URL=https://cdn.example.com
```

**The secret is encrypted at rest** (AES-256-GCM, key derived from `SECRET_KEY_BASE`) and is never
returned by the API — reads show a mask. Rotating `SECRET_KEY_BASE` makes the stored secret
undecryptable; re-enter it in the Studio.

Switching backends is safe: `_files.storage_backend` is stored **per row**, so files uploaded while
the local backend was active stay readable after you switch to S3.

---

## Direct upload (skip the server)

When S3/R2 is configured, the browser can `PUT` straight to the bucket so image bytes never touch
the app:

1. `POST /api/files/presign` `{ filename, size, mime }` → a pending row and a presigned `PUT`
   (`content-type` and `content-length` are signed to fixed values).
2. Upload the file to the returned URL with the returned headers.
3. `POST /api/files/:id/complete` → the server checks the object's size, magic bytes and dimensions,
   generates the eager variants and marks it ready. A failure deletes the object and the row.

With the TypeScript SDK this is one call:

```ts
const file = await client.files.uploadDirect(input.files[0], {
  collectionName: 'posts',
  fieldName: 'cover'
});
```

Abandoned pending uploads are cleaned up automatically (`LAZYPOCK_PENDING_UPLOAD_TTL_MS`, default 1 h).

---

## The Studio media library

**Media** (top navigation) lists every upload with thumbnails, filename search and paging. Select a
file to see its URL and metadata, or delete it — deleting a file that a record still uses asks for
confirmation first.

The same picker powers **every** file input:

- **Richtext fields** — the 🖼 toolbar button opens **Insert image** with two tabs: **Library** (pick
  an existing image) and **Upload** (drop or choose files; the upload is used immediately). Images
  are embedded as the `content` preset, never the original. Pasting or dropping an image into the
  editor uploads it the same way.
- **`file` / `multi_file` fields** — the same modal, multi-select when the field allows it.

---

## References, deletion & cleanup

Richtext content is plain Markdown, so Lazypock scans it for file ids and records them in
`_file_refs` whenever a record is created or updated. That drives three behaviours:

- **Delete guard** — `DELETE /api/files/:id` returns `409` with the list of records/fields that use
  the file. A superuser can force it with `DELETE /api/files/:id?force=true`.
- **Editor cleanup** — images uploaded from the editor that never end up in a saved record are
  deleted after `files.unattached_ttl_ms` (default 24 h). Library uploads are never auto-deleted.
- **URL rewriting** — after moving to a CDN domain:

```bash
lazypock content rewrite-urls --from /api/files --to https://cdn.example.com --dry-run
```

Deletions are **database-first**: the `_files` row is removed immediately (and the record no longer
points at it), an `_file_deletions` outbox entry is written in the same transaction, and the reaper
removes the objects afterwards — retrying with backoff if the bucket is unreachable.

---

## Operations

```bash
lazypock files reap                 # process the deletion outbox + stale pending uploads
lazypock files regen --preset thumb # regenerate variants (e.g. after changing a preset)
lazypock files reconcile --dry-run  # report orphaned local objects
lazypock files migrate --to s3 --delete-local   # move local files to S3 (resumable)
lazypock files trim                 # enforce LAZYPOCK_VARIANT_CACHE_MAX
```

`GET /api/health` reports the deletion-queue depth and the image-queue state, which is the quickest
way to see whether cleanup is stuck or images are backing up.

---

## API reference

| Endpoint | Access | Description |
| --- | --- | --- |
| `POST /api/files` | authenticated | Multipart upload (through the app) |
| `POST /api/files/presign` · `POST /api/files/:id/complete` | authenticated | Direct upload (S3/R2) |
| `GET /api/files` | superuser | Library list (`mime`, `q`, `page`, `perPage`) |
| `GET /api/files/:id` | public / rules | Original file |
| `GET /api/files/:id/scale/:preset_or_size` | public / rules | Preset variant or arbitrary size |
| `GET /api/files/:id/thumbs/:size` | public / rules | Legacy thumbnail |
| `DELETE /api/files/:id` | superuser | Delete (`409` if referenced; `?force=true`) |
| `GET/PATCH /api/settings/storage` | superuser | S3/R2 configuration (secret is masked) |
| `POST /api/settings/storage/test` | superuser | Real upload round-trip against the bucket |

A file record looks like this:

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
    "content": "https://cdn.example.com/lazypock/sunset/content.webp"
  }
}
```

---

## Troubleshooting

| Symptom | Likely cause |
| --- | --- |
| `413` on a 6 MB upload | the effective `max_size` (default 10 MB) or `LAZYPOCK_UPLOAD_MAX_MB` |
| `400` "contents do not match the filename extension" | the bytes are not really that format (check the source file) |
| `422` "exceeds the maximum allowed megapixels" | a very large image — raise `upload.max_megapixels` or downscale before upload |
| No `thumbs`/`variants` in the response | ImageMagick is not installed, the file is not an image, or `LAZYPOCK_THUMBNAILS=0` |
| `503` with `Retry-After` | the image queue is busy — retry, or raise `LAZYPOCK_IMAGE_CONCURRENCY` |
| Images 404 after switching to S3 | the bucket keys/prefix or public URL are wrong — run **Test connection** |
| An image does not appear in the editor | it is still generating (lazy preset) — the URL works on the next request |
| `409` when deleting from the library | the file is referenced by a record; the response lists where |
