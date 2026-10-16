# LazyPock Backup/Restore Scaling Plan (v2 — source-verified)

**Status:** Phases 1–4 **implemented** (see the status table below); Phases 5–7 still deferred.
**Verified against:** `dd95b458` = `v0.13.0`.
**Scope:** `gnuzd/lazypock` core (Elixir/Phoenix), Studio (SvelteKit), `docs/` (docs site), CLI.
**Target:** DBs growing toward ~10 GB and beyond, with individual records >100 MB stored inline in an
**editor** field's column value (confirmed).

## Implementation status

| Phase | Scope | State |
| --- | --- | --- |
| 1 | Path-scoped body limit, size-gated undo snapshot, preflight | ✅ `LazypockWeb.BodyParser`, `Backup.preflight/0` |
| 2 | Streaming NDJSON archive export | ✅ `Backup.export_stream/1`, `GET /api/export/archive` |
| 3 | Streaming archive import + batched upserts + 3 atomic modes | ✅ `Backup.restore_archive/3`, `GenericRecord.restore_many/3` |
| 4 | File-based undo checkpoint, confirmation gate, Neon notice, streaming prune | ✅ `Backup.rollback/0`, temp-table pruning, §4.5 items |
| 5 | File blobs in the archive | ⛔ deferred (needs the D3 destination decision + real S3) |
| 6 | Scheduled Cron backups | ⛔ deferred |
| 7 | Incremental backups | ⛔ deferred |

Deliberate deviations from the draft, found during implementation:

- **§4.2 isolation.** `SET TRANSACTION ISOLATION LEVEL REPEATABLE READ` is issued only when the export
  owns a fresh transaction. It is skipped when the caller is already in a transaction *or* the repo
  runs on the Ecto SQL sandbox (whose transaction is connection-level, so `Repo.in_transaction?/0` is
  false). Issuing it there both fails and aborts the transaction.
- **§4.3 batching.** Batches are grouped by shared column set, so a record omitting a column never
  nulls it out (matching the single-record `restore/2` semantics).
- **§4.4 pruning.** The keep-set is loaded into a `TEMP TABLE` and pruned with `NOT EXISTS`, instead of
  a MapSet of ids — pruning a 10 GB database must not hold every id in memory. `stream_ids/2` became
  unnecessary and was not added.
- **Studio progress.** Upload/download progress uses XHR against `/api/export/archive` and `POST
  /api/import`, reading the bearer token from the SDK's own auth store. Per-collection counts for an
  *archive* are reported after import rather than listed beforehand, because listing them client-side
  would mean reading the zip in the browser.

---

> **Original draft below.** The v1→v2 changelog in §0 and the coverage analysis in §4.8 are retained
> as the reasoning record; the status table above is authoritative for what shipped.
>
> **v2 is a rewrite, not an edit.** Three claims in v1 were factually wrong (one was v1's only claimed
> backend fix), and v1 missed a **published machine-readable compatibility contract** that shipped 4
> commits before v1's baseline. Every claim below carries a `file:line` reference; §0 records what
> changed and why.

---

## 0. Verification log (v1 → v2)

### 0.1 Claims re-verified as correct

| v1 claim | Evidence |
| --- | --- |
| `Plug.Parsers` has no explicit `:length` → 8 MB default body cap | `core/lib/lazypock_web/endpoint.ex:39` |
| Export materializes the whole DB in RAM | `core/lib/lazypock/backup.ex:48-68` → `GenericRecord.all/1` = unbounded `SELECT *` (`core/lib/lazypock/schemas/generic_record.ex:62-63`) |
| Import runs in one global transaction | `backup.ex:160-176`(`restore_atomically/3`) |
| Undo snapshot = a second full export serialized into JSONB, 5 kept | `backup.ex:444-450`, `:452-463`, `:372` (`@kept_snapshots 5`) |
| Per-row round-trips; `prune_records_not_in/1` loads everything | `backup.ex:354-360`, `:490-500` |
| No cross-collection consistency snapshot | `export/0` opens no transaction |
| Uploaded file blobs are out of scope | `export/0` never reads `_files` |
| Import accepts envelope, bare list, and PocketBase 23+ shape | `backup.ex:137-146`; `core/test/lazypock/backup_view_test.exs` |
| Studio "Restore" and "Import" already share one backend call | `core/lib/lazypock_web/controllers/settings_controller.ex:285`; `router.ex:110` |
| Studio pages are 268 / 300 lines, both `POST /api/import`, both mount `UndoImportButton` | `studio/src/routes/(app)/settings/backups/+page.svelte:105,268`; `.../import/+page.svelte:142,299` |
| `deleteMissing` defaults differ (`false` vs `true`) | `backups/+page.svelte:16`; `import/+page.svelte:21` |
| Cron has exactly 3 actions; no CLI cron subcommand | `core/lib/lazypock/cron.ex:18`, `cron/runner.ex:50-52`, `core/lib/lazypock/application.ex:13-40` |
| Server-side cursor streaming is genuinely available | Postgrex `handle_declare` (`deps/postgrex/lib/postgrex/protocol.ex:548`); `Ecto.Adapters.SQL.stream/4` supports `:max_rows` |
| The `25P02` fix and its tests exist | `b3a70b1`; `core/test/lazypock/backup_rollback_test.exs` |

### 0.2 v1 errors (corrected in this document)

**E1 — v1's only claimed backend fix was wrong.** v1 §4.5 item 1 asserted the CLI restore takes no
undo snapshot because `application.ex:72` calls `Backup.restore(payload, true)` "with no `opts`".
Wrong: `restore/2` resolves to `restore/3` with `opts = []`, and `snapshot? = Keyword.get([], :snapshot, true)`
is **`true`** (`backup.ex:135,146-158`). `backup_rollback_test.exs:46` asserts precisely this —
`Backup.restore(payload)` records `Backup.last_snapshot()`. The CLI restore **is** undoable.

→ Consequence: v1's §4.5 work item 1 is deleted. The real CLI/HTTP asymmetries are different and
smaller (§4.5).

**E2 — dangling `A13` references.** v1 §4.6, Phase 6, and changelog item 12 cite "A13", which never
existed (assumptions stopped at A12). Removed; a real A13 is added here (§1).

**E3 — the isolation API does not exist.** v1 §4.2 step 1 wrote
`Repo.transaction(fn -> ... end, isolation: :repeatable_read)`. Postgrex's `handle_begin` hardcodes
`"BEGIN"` and reads only `:mode` (`deps/postgrex/lib/postgrex/protocol.ex:610-620`); there is no
`isolation`/`isolation_level` plumbing for Postgres repo transactions (the `:isolation` you may
remember is **sandbox-only**, `deps/ecto_sql/lib/ecto/adapters/sql/sandbox.ex:562`; `:isolation_level`
is **TDS-only**). Corrected in §4.2.

### 0.3 v1 gaps (added in this document)

- **G1** — per-collection transactions silently redefine `atomic: true`, and break `rollback/0`.
- **G2** — "reuse the existing Files/S3 config" is not implementable: there is no global Files config,
  and `Files.Adapters.S3` is a stub.
- **G3** — Phase 1 would break Phase 4's `user_snapshot/0` if `export/0` were replaced.
- **G4** — changing `GET /api/export`'s response shape is a breaking API change (v1 claimed the
  opposite).
- **G5** — `prune_records_not_in/1` was never assigned to a phase, so rollback still OOMs at 10 GB.
- **G6 (new, found on fetch)** — a **published machine-readable compatibility contract** exists and
  v1 never mentions it. See §0.4.

### 0.4 The contract v1 missed (why this matters most)

Commit `1f2180c1` ("docs(ai): publish the backup/import JSON format for AI assistants", merged in
`ee39f643` / PR #130) deliberately published a frozen, crawler-friendly contract:

| Artifact | Path | Content |
| --- | --- | --- |
| Format reference | `docs/src/routes/backup/+page.md` (208 lines) | "export your whole database … to **one JSON file**"; canonical envelope; the importer is "forgiving" |
| JSON Schema (draft-07) | `docs/static/backup.schema.json` (148 lines) | `$id: https://lazypock.gnuzd.dev/backup.schema.json`, required `{ "collections": [...] }`, description: *"Generate this shape with `GET /api/export`"* |
| LLM index | `docs/static/llms.txt` | "Endpoints: `GET /api/export` (download), `POST /api/import` (restore)" |
| Crawler allowlist | `docs/static/robots.txt` | explicit allow for AI crawlers |
| Sidebar entry | `docs/src/lib/nav.ts:73` | Reference → "Backup & import JSON" |

Two direct consequences:

1. **`GET /api/export` is a documented public contract.** v1 planned to make it return an NDJSON
   zip. That silently invalidates the published schema, the docs page, `llms.txt`, and every AI
   assistant or script that was *explicitly invited* to consume it. The v2 design keeps the JSON
   response byte-compatible and adds a **new** route for the archive (§4.1, §4.7).
2. **`deleteMissing` guidance already contradicts the Studio.** The published page says:
   *"`deleteMissing` (**destructive**) drops any user collection missing from the file. Leave it
   `false` when importing a partial file."* The Studio Import page defaults it to **`true`**
   (`import/+page.svelte:21`). v1 filed this as a cosmetic "pick a default" item; it is actually a
   destructive-by-default setting that contradicts shipped documentation. Escalated in §4.5.

### 0.5 Field-name fidelity — checked at the user's request (corrects `PRODUCTION_PLAN.md`)

The user flagged: *"Field names normalized to snake_case (LazyPock DDL requires lowercase)"*. That
line is **`PRODUCTION_PLAN.md:89`**. It is **incorrect for LazyPock's own backup/restore path**, though
it does describe a real behavior of a *different* tool:

| Path | Field-name behavior | Evidence |
| --- | --- | --- |
| `Backup.restore/3` — Studio Import/Backups, `POST /api/import`, CLI `restore` | **Verbatim.** `tagColor` stays `tagColor` | `backup.ex:595-601`; `settings_export_import_test.exs:464-466`; `FieldNames` moduledoc |
| `Lazypock.PocketBase.Importer` — `mix lazypock.import_pocketbase` (one-time PB→LZ migration, sqlite3) | **Normalizes** camelCase → snake_case | `importer.ex:368-369`, `:498-504`, `:582-586` |

The DDL does **not** require lowercase field names:

- `validate_field_name!/1` accepts `^[A-Za-z][A-Za-z0-9_]*$` and keeps mixed case (`ddl.ex:849-858`);
  columns are declared as quoted identifiers so Postgres preserves case.
- `Lazypock.Schemas.FieldNames` states the design: *"LazyPock keeps field names **verbatim** end to
  end … `fieldName` stays `fieldName`."*
- Only the **collection/table** name is lowercase-restricted (`^[a-z][a-z0-9_]*$`, `ddl.ex:802`,
  `collection.ex:96`). The docs page documents that for `name`, not for fields.

Verified empirically against a live `localhost:5432`: `dynamic_controller_test.exs` (including the
"verbatim field names (camelCase / snake_case)" block) = **31 passed**;
`settings_export_import_test.exs` + `backup_rollback_test.exs` = **36 passed**.

→ Two artifacts carry the wrong rationale and should be corrected independently of this plan:
`PRODUCTION_PLAN.md:89`, and the comments at `importer.ex:368-369` / `:499`. The importer's *behavior*
is a **separate decision** (§9 D14), not part of the large-DB work — changing it changes the API keys a
migrated PocketBase app sees. The `passwordHash → password_hash` alias for existing system columns
(`backup.ex:599,679,712-713`) must be preserved either way.

---

## 1. Assumptions

| # | Assumption | Why it matters | Confidence |
| --- | --- | --- | --- |
| A1 | Postgres-only deployment (one DB = one app) | Allows `COPY`, cursors, `REPEATABLE READ` snapshots | High |
| A2 | Self-hosted Burrito single binary; operator has host shell access | Backup destination is a host-local path | High |
| A3 | DB size trends toward ~10 GB and may keep growing | Rules out hard-coding a ceiling; design must be size-independent | Medium |
| A4 | Some records exceed 100 MB **inline in a column** (an `editor` field), not `_files` uploads | Drives per-line JSON encode/batch guards; `editor` is a real canonical field type (`docs/src/routes/backup/+page.md`) | High |
| A5 | File blobs are a **separate track** from large records | Phase 3 stays lower priority; not the critical path | High |
| A6 | Automatic undo is a **small-DB-only** guarantee; above the threshold, warn + require confirmation | Removes the need for any heavyweight automatic rollback mechanism | High |
| A7 | Streaming scales independently of total size (never materializes >1 record/batch) | The approach must not need re-tuning when 10 GB is exceeded | High |
| A8 | Neon is the primary host, but Neon API integration belongs to a future hosted platform; core may only *detect and inform* (no API key, no API call) | Keeps core provider-agnostic | High |
| A9 | Studio (non-technical) **and** CLI (developer) both need full manual backup/restore control | HTTP streaming is a first-class requirement, not a fallback | High |
| A10 | "Restore" and "Import" share one backend implementation | Already true (`settings_controller.ex:285`) — no backend work, only frontend dedup (§4.5) | High |
| A11 | Scheduled backups use LazyPock's own NDJSON mechanism (not `pg_dump`); off by default; explicit schedule; Studio-only; destination auto-selected | One format, one restore path, no new binary dependency | High |
| A12 | NDJSON-per-collection inside an archive is the default **export** format at every size, and Import must keep accepting the classic single-JSON payload | Previewability + a hard backward-compatibility requirement | High |
| A13 | **The published JSON contract is frozen**: `GET /api/export`'s JSON response shape, `/backup.schema.json`, `/backup`, and `llms.txt` must keep working. Any new format is *additive* (a new route/artifact), and docs/schema must be extended, not replaced | Directly protects the contract published in `1f2180c1` (§0.4) | High |
| A14 | **S3 is not a usable backup destination today.** `Files.Adapters.S3` is a stub — every callback returns `"S3 adapter not yet implemented"` (`core/lib/lazypock/files/adapters/s3.ex:15-52`), and there is no global Files config (backend is a per-row `_files.storage_backend`, resolved by `Adapter.for_backend/1`, `core/lib/lazypock/files/adapter.ex:44-45`) | v1's "S3 if configured, else local" is a no-op; backup destination must be a concrete local path decision | High |

---

## 2. Current failure points (verified)

All backup/restore traffic funnels through `core/lib/lazypock/backup.ex`, so fixes land once and
benefit Studio, CLI, and HTTP.

1. **8 MB transport wall.** `endpoint.ex:39-43` configures `Plug.Parsers` with no `:length`, so the 8 MB
   default applies to every route. Files above it fail with `413` before `Backup.restore/2` runs. The
   Studio pages also `FileReader.readAsText`-load the whole file into a JS string first
   (`backups/+page.svelte:~89`).
2. **Export loads the whole DB into RAM.** `export/0` calls `GenericRecord.all/1` per collection
   (unbounded `SELECT *`, `generic_record.ex:62-63`) and accumulates every row for every collection
   before `Jason.encode!/1` (CLI, `application.ex:48-50`) or `json(conn, payload)` (HTTP,
   `settings_controller.ex:262`). Peak RSS ≈ 2-4× on-disk size.
3. **Import is one global transaction** (`backup.ex:160-176`): long transaction, locks across DDL and
   inserts, WAL/temp growth, and the accumulated result carried through `Repo.rollback/2`.
4. **The undo snapshot doubles the cost.** `snapshot: true` (Studio default, forced at
   `settings_controller.ex:285`) runs a full `export/0` before the import (`user_snapshot/0`,
   `backup.ex:444-450`) and serializes it again into `_import_snapshots` JSONB, keeping
   `@kept_snapshots 5` (`backup.ex:372,452-463`).
5. **Per-row round-trips.** `restore_records/3` issues one `INSERT ... ON CONFLICT` per record
   (`backup.ex:354-360` → `GenericRecord.restore/2`, `generic_record.ex:239-296`).
   `prune_records_not_in/1` loads every record to diff against a keep-set (`backup.ex:490-500`).
6. **No cross-collection consistency snapshot.** Each `all/1` is its own query; a write mid-export
   yields a torn backup.
7. **File blobs are out of scope.** Only `_files` metadata exists in the payload; a restore onto a
   fresh host leaves records pointing at missing blobs.
8. **Undo does not survive the host.** Even after §4.4, the Tier-1 checkpoint is a local file; losing
   the host loses the checkpoint *and* the DB. This is the honest limit of A6.

---

## 3. Decision axes

| Axis | Options | Recommendation |
| --- | --- | --- |
| Scope per operation | schema-only / schema+data / schema+data+files | Studio *Export page* stays schema-only (it builds JSON client-side from `GET /api/collections`, `export/+page.svelte:31,71-77` — it never calls `/api/export`). `GET /api/export`, Backups, and CLI backup become schema+data; files in Phase 3 |
| Export format | one JSON document / NDJSON-per-collection in an archive | **Archive for the new route (A12); keep the existing JSON response on `GET /api/export` unchanged (A13)** |
| Transport | chunked HTTP / CLI-only | Both first-class (A9). Prefer temp file + `send_file` over `send_chunked/2` (see §4.2.6) |
| Memory | hold in RAM / stream to disk | Stream to disk |
| Transaction granularity | one global tx / per-collection / 3 explicit modes | **3 explicit modes**; keep `:batch` as the default (§4.3.5) |
| Undo | always-JSONB / size-gated file checkpoint / `pg_dump` | Size-gated, file-based, NDJSON (A6/A11/A12) |
| Scheduled backups | none / new Cron action | New `"backup"` Cron action, NDJSON-based, off by default, Studio-only, local destination (A11/A14) |
| Consistency | per-collection queries / `REPEATABLE READ` | One `REPEATABLE READ` transaction (§4.2.1) |
| Incrementality | full / `updated_at` deltas | Phase 5, lower priority |

---

## 4. Chosen approach

### 4.1 Archive shape and routes

```text
backup-<timestamp>.zip
├── manifest.json           # format version, LazyPock version, collection list, row counts, checksums
├── schema.json             # collection defs: id, name, type, schema, rules, options, hooks
├── data/<collection>.ndjson
└── files/                  # Phase 3 — omitted until then
```

- `manifest.json` enables format/version validation and progress ("posts": 812345) without opening
  every data file.
- `schema.json` is separable so schema-only operations keep working.
- Zip (not tar) so a single file can be downloaded and `manifest.json` peeked at without full
  decompression.

**Routes — additive, per A13 (this is the v1 correction):**

| Route | Today | v2 |
| --- | --- | --- |
| `GET /api/export` | `Backup.export/0` → JSON envelope (`router.ex:109`) | **Unchanged.** Still the canonical JSON, still schema-conformant |
| `POST /api/import` | envelope / bare list / PocketBase (`router.ex:110`) | **Unchanged for JSON bodies**; additionally accepts `multipart/form-data` with an archive file |
| `GET /api/export/archive` | — | **New.** NDJSON zip, streamed via a temp file |
| `GET /api/import/status`, `POST /api/import/rollback` | existing | unchanged |

Rationale: `/backup.schema.json`'s own description says *"Generate this shape with `GET /api/export`"*.
Adding a route costs one line; breaking that costs the published contract.

### 4.2 Export path — `Backup.export_stream/1`

1. **Consistency.** Open `Repo.transaction/2` and make the **first statement**
   `SET TRANSACTION ISOLATION LEVEL REPEATABLE READ` (optionally `READ ONLY`). Do **not** pass
   `isolation:`/`isolation_level:` to `Repo.transaction` — those are not plumbed for Postgres
   (E3). `REPEATABLE READ` is snapshot isolation: it does not block writers, so a long export does
   not stall the app; it does hold back autovacuum from reclaiming dead tuples until it finishes.
   Document that temporary bloat.
2. **Cursor.** Use `Ecto.Adapters.SQL.stream/4` with `:max_rows` (e.g. 500) instead of
   `GenericRecord.all/1`. Postgrex implements this with a real server-side cursor
   (`handle_declare`). The stream must be consumed inside the transaction's `fn`; the connection is
   held for its duration.
3. **Per-line encode.** `Jason.encode!/1` one row at a time and append to that collection's
   `.ndjson`. Never accumulate a collection in a list. This is what makes a >100 MB row tractable —
   one encode call, one write, move on.
4. **Files.** Write `data/*.ndjson` + `manifest.json` + `schema.json` to a scratch dir.
5. **Zip.** Then close the DB transaction (zip needs no DB) and produce the archive.
6. **Delivery.**
   - CLI: leave the file on disk, print the path (also support `-o -` to stdout).
   - HTTP: **prefer `send_file/3` over `send_chunked/2`.** Chunked responses have no
     `Content-Length`, so the browser cannot show download progress or support Range/resume — poor
     for a non-technical user downloading 10 GB. A temp file gives Content-Length and Range; delete
     it after response completion.
7. **Disk budget.** The scratch dir plus the archive is ≈2× the DB size. State this, or stream the
   zip as it is written to halve it.

### 4.3 Import path

1. **Upload.** Accept the archive via `multipart/form-data` streamed to a temp file. The 8 MB cap
   lives in the **endpoint-level** `Plug.Parsers`, which runs before the router, so it cannot be
   scoped with a router pipeline. Concrete mechanism (see §9 D8 for the choice):
   - replace `plug Plug.Parsers` in `endpoint.ex` with a path-aware wrapper that dispatches a raised
     `:length` **only** for `POST /api/import`, e.g.
     `Plug.Parsers.call(conn, Plug.Parsers.init(parsers: [...], length: ...))`; or
   - leave parsing to `Plug.Conn.read_body/2` in the controller for that route.
   Either way: raise the limit on that one path only.
2. **Validate.** Unzip to scratch; read `manifest.json` first for format version + row counts.
3. **Schema.** Apply `schema.json` first (unchanged, not scale-sensitive).
4. **Stream rows.** `File.stream!(path, [], :line)` + one `Jason.decode!/1` per line. One >100 MB
   line is fine (peak ≈1-2× that row); never load a whole file.
5. **Batch upserts.** Accumulate rows and issue one multi-row `INSERT ... ON CONFLICT (id) DO UPDATE`
   per batch.
   - **Cap by bind parameters, not rows.** Postgres binds an `Int16` parameter count: max **65 535**
     parameters per statement. `rows × columns` must stay under it — "500 rows" is fine for a 10-field
     collection and *fails* for a 200-field one. Cap as
     `batch_rows = min(configured, div(60_000, column_count))`.
6. **Transactional modes — explicit (this is the v1 G1 correction).** Replace the current boolean with
   three modes:
   - `atomic: :batch` — **default, today's semantics.** One transaction around the whole import; any
     failure rolls everything back. `backup.ex:160-176`.
   - `atomic: :per_collection` — each collection in its own transaction; a failure leaves earlier
     collections committed and continues (unless a caller wants halt-on-first-error).
   - `atomic: false` — current best-effort, no wrapping transaction (`backup.ex:222-229`).

   Keeping `:batch` as the default preserves the documented contract (`backup.ex:118-124`,
   `settings_controller.ex:279-283`). v1's "just scope it per collection" would have silently weakened
   the Studio's default all-or-nothing guarantee.
7. **Size-aware guard.** Before adding a decoded row to the batch: if it is large (start ~5 MB), flush
   the current batch and insert that row alone. **Track the size while buffering** (e.g.
   `:erlang.iolist_size/1`) — do **not** `byte_size(Jason.encode!(row))` as a probe, which
   double-encodes the one row this feature exists for.
8. **Progress.** Compare applied rows against `manifest.json` counts; render a real progress bar in
   Studio (A9), and a stderr line in CLI.
9. **`prune_records_not_in/1` must stream too** (G5). It currently calls `GenericRecord.all/1` per
   collection (`backup.ex:490-500`); at 10 GB it OOMs during rollback even after the checkpoint
   becomes a file. Compare ids via a streamed query rather than materializing the table.

### 4.4 Rollback — size-gated, file-based checkpoint; warn + confirm above the threshold

**Tier 1 (below the threshold) — automatic checkpoint, file-based:**

Replace `user_snapshot/0`'s full in-memory `export/0` + JSONB blob with `export_stream/1` writing an
NDJSON archive, and store only a **pointer** (path, timestamp, byte size) in `_import_snapshots`.
`rollback/0` reads the archive back through `Backup.restore/3`. This removes failure point #4 at every
size; a full second copy of the DB as a JSONB column was never a good idea even for small DBs.

Two caveats to settle before implementing:

- **Destination.** A14: there is no global Files config and S3 is a stub. `Files.Adapters.Local`'s root
  is `priv/uploads/` inside the release dir, i.e. **lost on redeploy**, and routing backups through
  `Files.Store` would create a `_files` row that appears in the Files UI and is fetchable via
  `GET /api/files/:id`. Recommendation: a dedicated backup directory (default
  `priv/backups/`, overridable), **not** a `_files` row, and not mixed with uploads. See §9 D3.
- **Durability.** A local Tier-1 checkpoint does not survive host loss (failure point #8). If "undo
  after a redeploy" matters, the checkpoint must live outside the release dir. See §9 D5.

`rollback/0` must pin `atomic: :batch` — it wraps `restore/3` in its own transaction and relies on
batch-wide atomicity (`backup.ex:402-436`); with per-collection commits it could return an error while
having already committed part of the undo.

**Above the threshold — no automatic checkpoint, explicit confirmation:**

1. Do not attempt any automatic snapshot (it would cost time/space proportional to DB size — exactly
   the cost being avoided).
2. **Studio:** a distinct warning + confirm control: *"This import is large. Automatic undo will not
   be available. Proceed?"* — an explicit action, not just closing a dialog.
3. **CLI:** print the warning to stderr and require either an interactive Y/N or a required flag
   (e.g. `--no-undo-checkpoint`) for non-interactive use. Neither silently proceeding nor refusing
   outright is acceptable.
4. **Neon notice (A8):** if the host matches `*.neon.tech` (heuristic) or an explicit config flag is
   set, append: *"This looks like a Neon-hosted database — you can use a branch or point-in-time
   restore as an alternative safety net."* Informational only: no API call, no `NEON_API_KEY`, no
   functional dependency.
5. The import then proceeds per §4.3 regardless of whether a checkpoint was taken.

**Neon branching/PITR as an automated integration stays out of core** (A8) — reserved for a future
hosted-platform layer.

### 4.5 "Restore" vs "Import" — corrected

v1's premise was right (they are one operation) but its conclusion was wrong: the backend was
**already** unified before this plan existed, and the CLI snapshot "gap" does not exist (E1). What
actually needs doing:

| # | Item | Detail |
| --- | --- | --- |
| 1 | **Fix the destructive default.** Flip Studio Import's `deleteMissing` default to `false` (`import/+page.svelte:21`) | The published docs already say *"Leave it `false` when importing a partial file."* Today the page defaults to `true`, so pasting a partial JSON drops every collection not in the payload. The diff UI does surface removed collections (`import/+page.svelte:114,235`) and a password is required, so it is not silent — but it is destructive-by-default against shipped guidance. **User-visible behavior change → changelog entry.** |
| 2 | **De-duplicate the Studio frontend.** Extract file parsing, the `POST /api/import` call, and result handling into one composable (e.g. `studio/src/lib/composables/useImportRestore.svelte.ts`) | Real duplication: 300-line and 268-line pages each reimplement it (`import/+page.svelte:142`, `backups/+page.svelte:105`). Keep the genuinely different UX (Import: paste JSON + added/changed/removed diff; Backups: file-upload only + flat counts) |
| 3 | **Align the CLI with HTTP policy** (`application.ex:72`) | Not a snapshot bug (E1). Real asymmetries: CLI hardcodes `delete_missing = true` (HTTP: client-chosen) and has no password/confirmation gate. Apply the same threshold + confirmation flow from §4.4 |
| 4 | **No change to `Backup.restore/3`** for this item | Already correct for both callers; the only backend change is threshold gating (§4.4) |

The Backups page keeps its "Restore" section exactly as today; nothing is removed from the UI.

### 4.6 Scheduled automatic backups — new `"backup"` Cron action

LazyPock already has a scheduler (`core/lib/lazypock/cron.ex`, `_crons`, Studio Settings → Cron with
expression validation and next-run preview) supporting three actions: `@actions ~w(http sql hook)`
(`cron.ex:18`, dispatch at `cron/runner.ex:50-52`). Add a fourth.

```json
{
  "name": "Nightly backup",
  "expression": "0 2 * * *",
  "timezone": "Asia/Ho_Chi_Minh",
  "action": "backup",
  "config": { "retention": 14 }
}
```

- **Mechanism:** on fire, run `Backup.export_stream/1` — the same Phase 1 path. One format, one
  restore path (ordinary Import), no `pg_dump`/`pg_restore` binary dependency.
- **No `destination` field.** Destination is not per-job configurable.
- **Destination today is local only** (A14): S3 is an unimplemented stub. Write the "S3 if configured,
  else local" abstraction so it activates when S3 lands, but ship local-first and say so.
- **Constraints:** off by default (no migration creates a job); explicit `expression` required;
  Studio-only (CLI has no cron subcommands — keep it that way, `application.ex:13-40`); no new
  backup-storage env vars beyond a single backup directory.
- **Retention:** keep last N, pruned after each successful run.
- **Failure visibility:** a silently failing backup is worse than none. Route failures through
  `Lazypock.Audit.record/3` (`core/lib/lazypock/audit.ex:21`) and the existing mailer
  (`core/lib/lazypock/mailer.ex`, `emails.ex`).
- **UI:** add `"backup"` to the Cron action selector and its `'http' | 'sql' | 'hook'` union
  (`studio/src/routes/(app)/settings/cron/+page.svelte:20,118,197,259-275`), reusing the existing
  expression picker; the form shows only a retention field.

### 4.7 Compatibility & documentation obligations (new, per A13)

Every PR in this plan must state its effect on the published contract:

| Obligation | Action |
| --- | --- |
| `GET /api/export` response | **Must not change shape.** New format goes to a new route |
| `docs/static/backup.schema.json` | Keep valid for the canonical envelope. If the archive flow introduces anything importable-but-not-schema-valid, document it in `$comment` or a second schema |
| `docs/src/routes/backup/+page.md` | Add an "archive format" section describing the new export/import route, the `manifest.json`/`schema.json`/`data/*.ndjson` layout, and that the canonical JSON still works |
| `docs/static/llms.txt` | Add the archive route to the endpoint list |
| `deleteMissing` guidance | Already correct; make the Studio agree with it (§4.5 item 1) |
| Version banner | Add a `format_version` in `manifest.json` from day one so future format changes are detectable |

### 4.8 Coverage against the stated goal

Stated goal: *a JSON document can no longer hold the DB — make export/import and backup/restore work
at ~10 GB with >100 MB records.* Where each piece is load-bearing:

| Requirement | Covered by | Load-bearing? |
| --- | --- | --- |
| Remove the 8 MB transport wall | Phase 1.1 | **Yes** — nothing works without it |
| Export without RAM blowup | Phase 2 (cursor + NDJSON + temp file) | **Yes** |
| Import without RAM blowup + usable throughput | Phase 3 (line streaming + batched upsert + param cap) | **Yes** |
| Import does not OOM via the *undo snapshot* | Phase 4 (`user_snapshot/0` → file checkpoint) | **Yes** — `snapshot: true` is the Studio default (`settings_controller.ex:285`), so without this the import still doubles memory at **every** size |
| Consistent point-in-time export | Phase 2.1 | **Yes** |
| Rollback still correct after batching | Phase 3.6 (`:batch` pinned) + Phase 4 | **Yes** |
| `prune_records_not_in/1` does not OOM on rollback | Phase 4 (G5) | **Yes** |
| Existing single-JSON import keeps working | A12/A13 + contract test | **Yes** (regression guard) |
| Studio **and** CLI both usable | Phases 2-4 | **Yes** (A9) |

**The one functional gap for a *complete* backup: uploaded file blobs (`_files`).** Only the metadata
row is exported today, so restoring onto a fresh host leaves records pointing at missing files
(failure point #7). That is **Phase 5**, deferred under A5/A14. The stated >100 MB values are inline
`editor` column data, so this does not affect the large-DB problem — but if "true backup/restore" must
include uploads, Phase 5 is required, and it is blocked on the destination decision (D3) and on real S3.

**Not required for this goal** (safe to defer or drop): §4.6 scheduled Cron backups (Phase 6); A8 Neon
detection; §4.5 frontend dedup; the `deleteMissing` default flip (keep it only as a standalone safety
fix); Phase 7 incrementality. §4.7 docs work is required only to the extent the new archive route needs
documenting — the canonical JSON contract is untouched either way.

**Minimal scope that fully solves the stated problem: Phase 1 → Phase 4.** Add Phase 5 only if backups
must include uploaded files.

---

## 5. Phased implementation plan

Ordering is deliberate: v1's Phase 1→4 sequencing broke `user_snapshot/0` (G3).

### Phase 0 — Decisions + baseline (no shipping code)
- Resolve §9 decisions.
- Confirm numeric defaults: streaming threshold, batch size, single-row guard (start 5 MB), retention.
- **Baseline test run** with Postgres up — ✅ **recorded**: `dd95b458` / v0.13.0,
  `mix test` = **718 passed (6 properties, 712 tests), 0 failures** (Postgres on `localhost:5432`).
  Any claim that a later phase regresses behavior is measured against this.
- **No manual version bump** — release-please owns versions (`release-please-config.json`,
  `release-please-manifest.json`). Use `feat:`/`fix:` conventional commits.

### Phase 1 — Quick wins (ship independently; these are the real risk reducers)
1. Path-scoped raised `:length` + temp-file upload for `POST /api/import` (§4.3 step 1).
2. Size-gate / opt out of `snapshot: true` (`settings_controller.ex:285`) — alone removes failure
   point #4's worst case.
3. Pre-flight `pg_total_relation_size` check returning an actionable error instead of an OOM.

### Phase 2 — Export streaming (additive)
- Add `Backup.export_stream/1` (§4.2) **and keep `export/0` working** as a thin JSON wrapper so the
  undo path and `GET /api/export` are untouched (G3, A13).
- Add `GET /api/export/archive` (temp file → `send_file`).
- CLI `backup` gains the streaming path.
- Docs: extend `/backup` + `llms.txt` (§4.7).
- **Test:** round-trip a large-row collection; separately import a plain PocketBase-style single-JSON
  file to prove no regression (`backup_view_test.exs` covers the shapes).

### Phase 3 — Import streaming + batching
- Multipart temp-file upload; NDJSON line streaming; batched upsert with the **parameter cap**.
- Three explicit `atomic` modes; `rollback/0` pins `:batch`.
- Size-aware guard by tracked byte size.
- Progress reporting to Studio + CLI.
- **Test:** round-trip a Phase 2 archive; extend `core/test/lazypock/backup_rollback_test.exs` (not a
  new file) to cover all three modes and mid-import failure, per the `25P02` conventions.

### Phase 4 — Rollback rework
- Tier-1 file-based checkpoint via `export_stream/1`; pointer only in `_import_snapshots`.
- Threshold + explicit confirmation (Studio modal, CLI prompt/flag).
- Neon host heuristic + notice.
- **Stream `prune_records_not_in/1`** (G5).
- Configurable `LAZYPOCK_IMPORT_UNDO_MAX_MB`, `LAZYPOCK_IMPORT_UNDO_KEEP`.
- §4.5 items 1-3.
- Docs: threshold, warn-and-disable behavior, Neon notice, durability caveat.

### Phase 5 — File blobs
- Add `files/` to the archive; restore blobs to the configured adapter.
- **Blocked on the destination decision** (§9 D3) and effectively on S3 landing for S3 users.
  If Phase 0 confirms the >100 MB values are uploads rather than inline columns, promote ahead of
  Phase 3.

### Phase 6 — Scheduled backups
- `"backup"` Cron action (§4.6), retention, Audit/mailer failure path, Studio selector.

### Phase 7 — Incremental backups + docs (lower priority)
- `updated_at` deltas; README size matrix.

---

## 6. Testing strategy

- **Large single row:** a configurable fixture (e.g. `LAZYPOCK_TEST_BIG_ROW_MB`, default 120) rather
  than v1's "50 × 150 MB = 7.5 GB", which is not CI-feasible. Assert on **peak process memory**
  during export/import, not just success.
- **Many small rows:** separate fixture for the batching path, plus a wide collection (>130 columns)
  to exercise the 65 535-parameter cap.
- **Regression suite:** extend `core/test/lazypock/backup_rollback_test.exs`; keep the `25P02`
  coverage; add cases for all three `atomic` modes and for `rollback/0` under each.
- **Contract tests:** assert `GET /api/export` output still validates against
  `docs/static/backup.schema.json` (the repo already has `ajv` available in `docs/`'s toolchain) —
  this is the guardrail A13 needs.
- **Load test:** disposable env sized near the real worst case; measure BEAM peak RSS for export and
  import before making streaming the default.
- Postgres is required and is available (`localhost:5432`); the baseline above was captured on it
  (`docker-compose.yml` is present for a clean environment).

---

## 7. What is explicitly NOT in this plan

- `pg_dump`/`pg_restore` as a dependency (dropped in v1's round 3; confirmed dropped here).
- Any Neon API integration in core (A8).
- Removing the Studio "Restore" section (A10).
- Changing `GET /api/export`'s response shape (A13).
- CLI subcommands for cron management (A11).
- A separate backup-storage config surface beyond one backup directory (A14).

---

## 8. Changelog v1 → v2

1. **E1 corrected:** the CLI restore already takes an undo snapshot; v1's only claimed backend fix was
   based on misreading Elixir default arguments. §4.5 rewritten; the real items are the destructive
   `deleteMissing` default, frontend duplication, and CLI policy alignment.
2. **E2 corrected:** dangling `A13` references removed.
3. **E3 corrected:** `isolation: :repeatable_read` is not a Postgres Ecto option; use a leading
   `SET TRANSACTION ISOLATION LEVEL REPEATABLE READ`.
4. **G1 corrected:** three explicit `atomic` modes; `:batch` stays the default; `rollback/0` pins it.
5. **G2 corrected:** S3 is a stub and there is no global Files config → A14; backup destination is a
   concrete local-path decision, not "reuse existing config".
6. **G3 corrected:** Phase 2 keeps `export/0` as a shim so Phase 4's undo path never breaks.
7. **G4 corrected:** `/api/export`'s JSON shape is frozen; the archive gets a new route.
8. **G5 added:** `prune_records_not_in/1` must stream (now Phase 4).
9. **G6 added (found on fetch):** the published AI-fetchable contract
   (`docs/src/routes/backup/+page.md`, `docs/static/backup.schema.json`,
   `docs/static/llms.txt`, commit `1f2180c1`) is a frozen compatibility surface → new A13 and §4.7.
10. **Technical corrections:** 65 535 bind-parameter cap for batch sizing; no double-encode in the
    size guard; `send_file` over `send_chunked/2`; disk budget stated; concrete `Plug.Parsers`
    scoping mechanism; realistic test fixtures; release-please owns the version.
11. **Escalated:** `deleteMissing: true` on Import is destructive-by-default and contradicts shipped
    docs — a behavior change, not a cosmetic default.
12. **Added §0.5 + D14 (user-raised):** `PRODUCTION_PLAN.md:89` (*"LazyPock DDL requires lowercase"*)
    is wrong for LazyPock's own backup/restore path, which keeps field names **verbatim**
    (`backup.ex:595-601`, `FieldNames`, `ddl.ex:849-858`, 67 tests run green). Only the
    **collection/table** name is lowercase-restricted. The snake_case normalization is confined to the
    PocketBase migration tool (`importer.ex:368-369,499`), which repeats the same wrong rationale.
13. **Added §4.8:** explicit coverage table answering "does this solve large-DB export/import?" —
    **yes, Phases 1-4**, with uploaded file blobs (Phase 5) as the only functional gap for a complete
    restore, and the Cron/Neon/frontend items marked as deferrable.

---

## 9. Open decisions — required BEFORE implementation

These are the "one more round" items. Each changes code or a public contract, so each needs an
explicit answer.

| # | Decision | Options | Recommendation |
| --- | --- | --- | --- |
| D1 | Archive container | zip / tar+zstd / plain dir + manifest | zip (downloadable single file, random-access `manifest.json`) |
| D2 | How to expose the archive over HTTP | new route / content negotiation on `Accept` on `/api/export` / break `/api/export` | **New route** — keeps A13 intact. Breaking `/api/export` invalidates the published schema + `llms.txt` |
| D3 | Backup/checkpoint destination path | `priv/backups/` / `LAZYPOCK_BACKUP_DIR` / reuse `priv/uploads/` | Dedicated dir, overridable; **not** `priv/uploads`, **not** a `_files` row |
| D4 | Tier-1 undo storage | file-based via `export_stream/1` / keep JSONB below the threshold | File-based (removes failure point #4 at all sizes) — but it is more work than keeping JSONB for small DBs; confirm scope |
| D5 | Must a Tier-1 checkpoint survive a redeploy / host loss? | yes (needs a path outside the release dir) / no | Decide explicitly; today failure point #8 makes undo host-local |
| D6 | `atomic` modes | `:batch` (default) + `:per_collection` + `false` / only rename/re-document | Three modes; `:batch` default; `rollback/0` pins `:batch` |
| D7 | Flip Studio Import `deleteMissing` default to `false`? | yes / no | **Yes** — it contradicts the published docs; requires a changelog entry |
| D8 | `Plug.Parsers` scoping mechanism | path-aware parser wrapper / manual `read_body` in the controller | Path-aware wrapper (keeps one parsing path) |
| D9 | Scheduled-backup retention + failure channel | keep-last-N; Audit only vs. Audit + email | Keep-last-N (default 14); Audit + email |
| D10 | Neon detection heuristic | `*.neon.tech` host match / explicit config flag / both | Both, informational only, with an opt-out env var |
| D11 | Update `docs/` (schema + page + `llms.txt`) in the same PR? | yes / separate follow-up | **Same PR** — an undocumented importable format is a support problem |
| D12 | Threshold + guard values | e.g. undo threshold 50 MB export, guard 5 MB, batch 500 rows | Start here; tune after Phase 2 measurements |
| D13 | Phase 5 (file blobs) in or out of this effort? | in / defer until S3 lands | **Defer** until D3 is settled and S3 is real. Required only if a backup must include uploads (§4.8) |
| D14 | PocketBase importer field names | keep snake_case normalization / keep verbatim like the rest of LazyPock | **Separate from this plan.** Unrelated to the large-DB goal; changes the API keys PB users see, so decide on its own merits |

---

## 10. Definition of done for "ready to implement"

- [ ] §9 D1-D13 answered.
- [ ] Phase 0 baseline `mix test` run recorded (Postgres up).
- [ ] Branch cut from fresh `origin/main` (`fix/`/`feat/`), one PR per phase.
- [ ] Every PR states its effect on the §4.7 contract table.
- [ ] A contract test asserting `GET /api/export` still validates against `backup.schema.json`.
