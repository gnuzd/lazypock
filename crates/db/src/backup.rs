//! Backup archives: the format, and the rules that make a restore safe.
//!
//! The format is documented in `docs/src/routes/backup/+page.md`, which is the
//! source of truth:
//!
//! ```text
//! backup.zip
//! ├── manifest.json            format, version, per-collection row counts
//! ├── schema.json              { "collections": [...] } — definitions, no records
//! ├── data/<collection>.ndjson  one JSON object per line, one line per record
//! └── files.ndjson              upload metadata (optional)
//! ```
//!
//! `manifest.json` is written **first** so a client can read it out of the zip
//! header without unpacking the archive, which is what Studio's import page does to
//! show a preview before uploading.
//!
//! # What makes a restore safe
//!
//! Two rules, enforced here rather than left to a client:
//!
//! 1. **An import never deletes.** A collection or record that the archive does not
//!    mention is left exactly as it is. Without this, importing a partial file
//!    would silently drop everything it did not contain — which is a real defect
//!    the documentation already records against the Studio import page, whose
//!    `deleteMissing` defaults to *true*.
//! 2. **An import never writes a system collection.** The `_`-prefixed tables are
//!    lazypock's own machinery: users, password hashes, signing keys, migration
//!    state. A restore that touched them could lock everyone out or corrupt the
//!    ledger, so they are refused and reported, not applied.
//!
//! Neither rule is a default or a flag. A destructive restore is a different
//! operation from a restore, and it is not this one.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The `format` value that identifies a lazypock archive.
pub const ARCHIVE_FORMAT: &str = "lazypock-archive";

/// The current archive format version.
pub const ARCHIVE_FORMAT_VERSION: u32 = 1;

/// The entry a client reads first, without unpacking the rest.
pub const MANIFEST_ENTRY: &str = "manifest.json";

/// The entry holding collection definitions.
pub const SCHEMA_ENTRY: &str = "schema.json";

/// The directory holding per-collection record data.
pub const DATA_PREFIX: &str = "data/";

/// The entry holding upload metadata.
pub const FILES_ENTRY: &str = "files.ndjson";

/// Whether a table belongs to lazypock rather than to the user.
///
/// The `_` prefix is the same convention schema reflection uses to decide what not
/// to adopt, so the two agree by construction.
pub fn is_system_collection(name: &str) -> bool {
    name.starts_with('_')
}

/// What an import will do to one collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportAction {
    /// The collection does not exist yet and will be created.
    Create,
    /// The collection exists and will be updated in place.
    Update,
    /// The collection is deliberately left alone, for the reason given.
    Skip(String),
}

/// A collection as an archive carries it.
///
/// `schema` and `fields` are both accepted because the published format uses
/// `schema` (PocketBase's older spelling) while this server's own wire format uses
/// `fields`; a file written by either is valid. A file written by *this* server's
/// Studio export carries **both**, because that export is the objects
/// `GET /api/collections` serves, and those serve the derived `schema` beside
/// `fields`. When both are there, `fields` wins: it is the spelling the API keeps,
/// and the `schema` beside it is derived from it and carries less (`id`,
/// `collectionId` and `sort_order` are not in it).
#[derive(Debug, Clone, Serialize)]
pub struct ArchiveCollection {
    /// Table name.
    pub name: String,
    /// `base`, `auth` or `view`.
    #[serde(default, rename = "type")]
    pub collection_type: Option<String>,
    /// Field definitions, under either spelling (see the type's docs).
    #[serde(default)]
    pub schema: Vec<Value>,
    /// Access rules.
    #[serde(default)]
    pub rules: Option<Value>,
    /// Type-specific settings.
    #[serde(default)]
    pub options: Option<Value>,
    /// Records to upsert, by `id`.
    #[serde(default)]
    pub records: Vec<Value>,
}

impl<'de> Deserialize<'de> for ArchiveCollection {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Hand-written rather than `alias = "fields"`, because an alias makes the two
        // keys mutually exclusive: `{"fields": [...], "schema": [...]}` is then read
        // as the same field twice and refused with "duplicate field `schema`", which
        // is exactly what this server's own collection payload contains.
        #[derive(Deserialize)]
        struct Wire {
            name: String,
            #[serde(default, rename = "type")]
            collection_type: Option<String>,
            #[serde(default)]
            schema: Option<Vec<Value>>,
            #[serde(default)]
            fields: Option<Vec<Value>>,
            #[serde(default)]
            rules: Option<Value>,
            #[serde(default)]
            options: Option<Value>,
            #[serde(default)]
            records: Vec<Value>,
        }

        let wire = Wire::deserialize(deserializer)?;
        let fields = wire.fields.unwrap_or_default();
        let schema = wire.schema.unwrap_or_default();

        Ok(ArchiveCollection {
            name: wire.name,
            collection_type: wire.collection_type,
            // A populated `fields` is the authority; an empty one is silence rather
            // than a definition of zero columns, so the `schema` beside it still
            // counts.
            schema: if fields.is_empty() { schema } else { fields },
            rules: wire.rules,
            options: wire.options,
            records: wire.records,
        })
    }
}

/// The canonical JSON an archive's `schema.json` holds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveSchema {
    /// Every collection in the archive.
    pub collections: Vec<ArchiveCollection>,
}

/// The archive's `manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Always [`ARCHIVE_FORMAT`].
    pub format: String,
    /// Format version, so an older reader can refuse a newer file.
    pub format_version: u32,
    /// The server that wrote it.
    #[serde(default)]
    pub lazypock_version: Option<String>,
    /// When it was written.
    #[serde(default)]
    pub created_at: Option<String>,
    /// Per-collection row counts, so progress needs no unpacking.
    #[serde(default)]
    pub collections: Vec<ManifestCollection>,
    /// Upload totals.
    #[serde(default)]
    pub files: Option<ManifestFiles>,
    /// Overall totals.
    #[serde(default)]
    pub totals: Option<ManifestTotals>,
}

/// One collection's line in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestCollection {
    /// Table name.
    pub name: String,
    /// `base`, `auth` or `view`.
    #[serde(default, rename = "type")]
    pub collection_type: Option<String>,
    /// Rows the archive holds for it.
    #[serde(default)]
    pub records: u64,
}

/// Upload totals in the manifest.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ManifestFiles {
    /// Objects the archive carries.
    #[serde(default)]
    pub total: u64,
    /// Their combined size.
    #[serde(default)]
    pub bytes: u64,
    /// Objects that were already missing when the backup was taken. Recorded rather
    /// than aborting the backup, so a missing object cannot cost you the archive.
    #[serde(default)]
    pub missing: u64,
}

/// Overall totals in the manifest.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ManifestTotals {
    /// Collections in the archive.
    #[serde(default)]
    pub collections: u64,
    /// Records in the archive.
    #[serde(default)]
    pub records: u64,
    /// Uploads in the archive.
    #[serde(default)]
    pub files: u64,
}

/// What an import would do, worked out before anything is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportPlan {
    /// One entry per collection in the archive, in archive order.
    pub actions: Vec<(String, ImportAction)>,
}

impl ImportPlan {
    /// Collections the import would create or update.
    pub fn writable(&self) -> impl Iterator<Item = &(String, ImportAction)> {
        self.actions
            .iter()
            .filter(|(_, action)| !matches!(action, ImportAction::Skip(_)))
    }

    /// Collections the import refuses to touch, with the reason.
    pub fn skipped(&self) -> impl Iterator<Item = &(String, ImportAction)> {
        self.actions
            .iter()
            .filter(|(_, action)| matches!(action, ImportAction::Skip(_)))
    }

    /// Whether any collection would be written.
    pub fn is_empty(&self) -> bool {
        self.writable().next().is_none()
    }
}

/// Decide what an import would do.
///
/// Pure, because the answer is the security-relevant part: it is asserted directly
/// rather than inferred from what a database looks like afterwards.
///
/// `existing` is every collection name currently in the database. Note what is
/// *not* done with it: names present there but absent from the archive produce no
/// action at all. That is the no-delete rule, and it is expressed by omission
/// rather than by a filter that could be forgotten.
pub fn plan(archive: &[ArchiveCollection], existing: &[String]) -> ImportPlan {
    let mut actions = Vec::with_capacity(archive.len());

    for collection in archive {
        if is_system_collection(&collection.name) {
            actions.push((
                collection.name.clone(),
                ImportAction::Skip(
                    "a system collection: an import never writes lazypock's own tables".to_string(),
                ),
            ));
            continue;
        }

        let action = if existing.iter().any(|name| name == &collection.name) {
            ImportAction::Update
        } else {
            ImportAction::Create
        };
        actions.push((collection.name.clone(), action));
    }

    ImportPlan { actions }
}

// ---------------------------------------------------------------------------
// Reading and writing the archive
// ---------------------------------------------------------------------------

use std::io::{Cursor, Read, Write};

use lazypock_core::error::{Error, Result};
use tokio_postgres::Client;

use crate::collections;
use crate::pool::db_error;

/// An archive as it is held in memory.
#[derive(Debug, Clone)]
pub struct Archive {
    /// The manifest, which identifies the format and lists what is inside.
    pub manifest: Manifest,
    /// Collection definitions.
    pub schema: ArchiveSchema,
    /// Records per collection, in the order the archive holds them.
    pub data: Vec<(String, Vec<Value>)>,
}

impl Archive {
    /// Rows the archive carries, per the data it actually holds.
    pub fn record_count(&self) -> usize {
        self.data.iter().map(|(_, records)| records.len()).sum()
    }

    /// The collections the archive would write, and how.
    pub fn plan(&self, existing: &[String]) -> ImportPlan {
        plan(&self.schema.collections, existing)
    }
}

/// The canonical single-document JSON a backup holds.
///
/// This is the shape `GET /api/export` serves and `docs/static/backup.schema.json`
/// describes: the definitions and their records together, rather than the archive's
/// split `schema.json` + `data/*` entries. It lives beside [`Archive`] so the two
/// cannot drift, and so a downloaded JSON backup and a pre-import snapshot are the
/// same document.
///
/// System collections never appear: [`export`] already leaves them out, because
/// `_superusers` holds password hashes and `_settings` holds the signing key.
pub fn to_payload(archive: &Archive) -> Value {
    let collections: Vec<Value> = archive
        .schema
        .collections
        .iter()
        .map(|collection| {
            let mut value = serde_json::to_value(collection).unwrap_or(Value::Null);
            let records = archive
                .data
                .iter()
                .find(|(name, _)| name == &collection.name)
                .map(|(_, records)| Value::Array(records.clone()))
                .unwrap_or_else(|| Value::Array(Vec::new()));
            if let Some(object) = value.as_object_mut() {
                // `ArchiveCollection.records` is filled by a JSON reader, not by
                // `export` (which keeps records in `data`), so put them back.
                object.insert("records".to_string(), records);
            }
            value
        })
        .collect();

    serde_json::json!({ "collections": collections })
}

/// Parse a single-document JSON payload into an [`Archive`].
///
/// Accepts the canonical `{ "collections": [...] }` envelope, a bare array of
/// collections, and either field spelling (`schema` or `fields`) — all three are
/// what a client or a PocketBase export can send. Kept beside [`to_payload`] so the
/// document this server writes is a document it reads.
pub fn archive_from_payload(payload: &Value) -> Result<Archive> {
    let envelope =
        match payload {
            Value::Array(_) => serde_json::json!({ "collections": payload }),
            Value::Object(map) if map.contains_key("collections") => payload.clone(),
            _ => return Err(Error::invalid(
                "a backup payload must be an object with `collections`, or an array of collections",
            )),
        };

    let mut schema: ArchiveSchema = serde_json::from_value(envelope)
        .map_err(|err| Error::invalid(format!("the backup payload is not valid: {err}")))?;

    // Records travel inline in the payload but live in `data` in memory, so split
    // them out the same way `read` does for a zip archive.
    let data: Vec<(String, Vec<Value>)> = schema
        .collections
        .iter_mut()
        .map(|collection| {
            (
                collection.name.clone(),
                std::mem::take(&mut collection.records),
            )
        })
        .collect();

    let collections: Vec<ManifestCollection> = schema
        .collections
        .iter()
        .map(|collection| ManifestCollection {
            name: collection.name.clone(),
            collection_type: collection.collection_type.clone(),
            records: data
                .iter()
                .find(|(name, _)| name == &collection.name)
                .map_or(0, |(_, records)| records.len() as u64),
        })
        .collect();
    let records = collections
        .iter()
        .map(|collection| collection.records)
        .sum();

    Ok(Archive {
        manifest: Manifest {
            format: ARCHIVE_FORMAT.to_string(),
            format_version: ARCHIVE_FORMAT_VERSION,
            lazypock_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            created_at: Some(now_rfc3339()),
            totals: Some(ManifestTotals {
                collections: schema.collections.len() as u64,
                records,
                files: 0,
            }),
            collections,
            files: Some(ManifestFiles::default()),
        },
        schema,
        data,
    })
}

/// Restore the database to a snapshot payload, **destructively**.
///
/// This is the rollback path, and it is the one place that removes data: a bad
/// import can have added a collection or a row, and undoing it means deleting what
/// the snapshot does not have. The additive [`import`] is untouched. System
/// collections are never dropped.
///
/// Rows are replaced with the tables' triggers disabled, so a foreign key cannot
/// fail on insert order. `ALTER TABLE ... DISABLE TRIGGER ALL` is transactional, so
/// a failure rolls the disable back with everything else and triggers can never be
/// left off.
pub async fn restore_snapshot(client: &mut Client, payload: &Value) -> Result<ImportReport> {
    let archive = archive_from_payload(payload)?;

    let wanted: std::collections::HashSet<String> = archive
        .schema
        .collections
        .iter()
        .map(|collection| collection.name.clone())
        .filter(|name| !is_system_collection(name))
        .collect();

    // Drop every user collection the snapshot does not know about: those are the
    // ones an import created.
    let existing: Vec<String> = collections::list(client)
        .await?
        .into_iter()
        .map(|collection| collection.name)
        .filter(|name| !is_system_collection(name))
        .collect();
    for name in &existing {
        if !wanted.contains(name) {
            collections::delete(client, name, true).await?;
        }
    }

    // Recreate any collection the snapshot has and the database has lost.
    let current: std::collections::HashSet<String> = collections::list(client)
        .await?
        .into_iter()
        .map(|collection| collection.name)
        .collect();
    for name in &wanted {
        if !current.contains(name) {
            create_collection(client, &archive, name).await?;
        }
    }

    let mut report = ImportReport {
        imported: Vec::new(),
        skipped: Vec::new(),
        errors: Vec::new(),
    };

    let names: Vec<String> = wanted.iter().cloned().collect();
    let transaction = client.transaction().await.map_err(db_error)?;
    for name in &names {
        let table = crate::ddl::quote_ident(name);
        transaction
            .batch_execute(&format!("alter table {table} disable trigger all"))
            .await
            .map_err(db_error)?;
    }
    for name in &names {
        let table = crate::ddl::quote_ident(name);
        transaction
            .batch_execute(&format!("delete from {table}"))
            .await
            .map_err(db_error)?;
    }
    for name in &names {
        apply_records(&transaction, &archive, name).await?;
        report
            .imported
            .push(imported_of(&archive, name, &ImportAction::Update));
    }
    for name in &names {
        let table = crate::ddl::quote_ident(name);
        transaction
            .batch_execute(&format!("alter table {table} enable trigger all"))
            .await
            .map_err(db_error)?;
    }
    transaction.commit().await.map_err(db_error)?;

    tracing::info!(
        collections = report.imported.len(),
        "restored an import snapshot"
    );
    Ok(report)
}

/// Build the archive for every user collection in the database.
///
/// **System collections are not exported at all.** Their definitions are created by
/// migrations, so a restore does not need them, and their rows are the opposite of
/// a backup: `_superusers` holds password hashes and `_settings` holds the signing
/// key. Shipping those in a file that gets emailed around would turn a convenience
/// into a credential leak.
pub async fn export(client: &Client) -> Result<Archive> {
    let mut manifest_collections: Vec<ManifestCollection> = Vec::new();
    let mut schema = ArchiveSchema {
        collections: Vec::new(),
    };
    let mut data = Vec::new();
    let mut total_records: u64 = 0;

    for collection in collections::list(client).await? {
        if is_system_collection(&collection.name) {
            continue;
        }

        // `to_jsonb(t)` renders the whole row, so Postgres decides how each type
        // becomes JSON. A hand-written match was wrong for `timestamptz` — which
        // every table here has, as `created` and `updated` — and would have been
        // wrong again for the next type somebody added.
        let rows = client
            .query(
                &format!(
                    "select to_jsonb(t) as row from {} t order by id",
                    crate::ddl::quote_ident(&collection.name)
                ),
                &[],
            )
            .await
            .map_err(db_error)?;

        let records: Vec<Value> = rows.iter().map(|row| row.get("row")).collect();

        manifest_collections.push(ManifestCollection {
            name: collection.name.clone(),
            collection_type: Some(collection.collection_type.as_str().to_string()),
            records: records.len() as u64,
        });
        total_records += records.len() as u64;

        schema.collections.push(ArchiveCollection {
            name: collection.name.clone(),
            collection_type: Some(collection.collection_type.as_str().to_string()),
            schema: collection
                .fields
                .iter()
                .map(|field| serde_json::to_value(field).unwrap_or(Value::Null))
                .collect(),
            rules: Some(collection.rules.clone()),
            options: Some(collection.options.clone()),
            records: Vec::new(),
        });

        data.push((collection.name.clone(), records));
    }

    let manifest = Manifest {
        format: ARCHIVE_FORMAT.to_string(),
        format_version: ARCHIVE_FORMAT_VERSION,
        lazypock_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        created_at: Some(now_rfc3339()),
        totals: Some(ManifestTotals {
            collections: manifest_collections.len() as u64,
            records: total_records,
            files: 0,
        }),
        collections: manifest_collections,
        // No upload metadata is exported yet; reported as zero rather than omitted,
        // so a reader can tell "none" from "not recorded".
        files: Some(ManifestFiles::default()),
    };

    tracing::info!(
        collections = schema.collections.len(),
        records = total_records,
        "exported a backup archive"
    );

    Ok(Archive {
        manifest,
        schema,
        data,
    })
}

/// The current time, RFC 3339, from the system clock.
fn now_rfc3339() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    crate::backup::format_rfc3339(seconds)
}

/// Format unix seconds as RFC 3339, in UTC.
pub fn format_rfc3339(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let time = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time / 3600,
        (time % 3600) / 60,
        time % 60
    )
}

/// The date for a count of days since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let march_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * march_month + 2) / 5 + 1) as u32;
    let month = if march_month < 10 {
        march_month + 3
    } else {
        march_month - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// Write an archive.
///
/// `manifest.json` is written **first**, because Studio reads it straight out of the
/// zip's first local file header to show an import preview without unpacking the
/// archive. Nothing else may be written before it.
pub fn write(archive: &Archive) -> Result<Vec<u8>> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));

    let stored = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        // A large file would mean a data descriptor; the format rules those out so a
        // client can trust the sizes in the header it peeks at.
        .large_file(false);

    start(&mut writer, MANIFEST_ENTRY, stored)?;
    write_json(&mut writer, &archive.manifest)?;

    start(&mut writer, SCHEMA_ENTRY, stored)?;
    write_json(&mut writer, &archive.schema)?;

    for (name, records) in &archive.data {
        start(&mut writer, &format!("{DATA_PREFIX}{name}.ndjson"), stored)?;
        // One record per line, written as it is read: never the whole collection in
        // one string, which is what makes a large collection tractable.
        for record in records {
            let line = serde_json::to_string(record)
                .map_err(|err| Error::internal(format!("cannot serialise a record: {err}")))?;
            writer
                .write_all(line.as_bytes())
                .and_then(|()| writer.write_all(b"\n"))
                .map_err(|err| Error::internal(format!("cannot write a record: {err}")))?;
        }
    }

    // No upload metadata yet, but the entry exists so a reader does not have to
    // treat its absence as a different format.
    start(&mut writer, FILES_ENTRY, stored)?;

    let cursor = writer
        .finish()
        .map_err(|err| Error::internal(format!("cannot finish the archive: {err}")))?;
    Ok(cursor.into_inner())
}

/// Start an entry in the archive.
fn start<W: Write + std::io::Seek>(
    writer: &mut zip::ZipWriter<W>,
    name: &str,
    options: zip::write::SimpleFileOptions,
) -> Result<()> {
    writer
        .start_file(name, options)
        .map_err(|err| Error::internal(format!("cannot write {name} to the archive: {err}")))
}

/// Write a JSON value to the archive.
fn write_json<W: Write>(writer: &mut W, value: &impl Serialize) -> Result<()> {
    let json = serde_json::to_vec(value)
        .map_err(|err| Error::internal(format!("cannot serialise the archive: {err}")))?;
    writer
        .write_all(&json)
        .map_err(|err| Error::internal(format!("cannot write the archive: {err}")))
}

/// Read an archive.
///
/// The manifest is checked before anything else is trusted: an archive from a
/// version this server does not understand is refused rather than partly applied.
pub fn read(bytes: &[u8]) -> Result<Archive> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes.to_vec()))
        .map_err(|err| Error::invalid(format!("not a readable archive: {err}")))?;

    let manifest: Manifest = read_json(&mut zip, MANIFEST_ENTRY)?;
    if manifest.format != ARCHIVE_FORMAT {
        return Err(Error::invalid(format!(
            "this is not a lazypock archive (format {:?}, expected {ARCHIVE_FORMAT:?})",
            manifest.format
        )));
    }
    if manifest.format_version > ARCHIVE_FORMAT_VERSION {
        return Err(Error::invalid(format!(
            "the archive is format version {} but this server understands {ARCHIVE_FORMAT_VERSION}; \
             a newer archive may contain things this version would silently drop",
            manifest.format_version
        )));
    }

    let schema: ArchiveSchema = read_json(&mut zip, SCHEMA_ENTRY)?;

    let mut data = Vec::new();
    for collection in &schema.collections {
        let entry = format!("{DATA_PREFIX}{}.ndjson", collection.name);
        let Some(text) = read_text(&mut zip, &entry)? else {
            // A collection with no data entry carries no records, which is legal:
            // `view` collections have none and a definition-only archive is valid.
            data.push((collection.name.clone(), Vec::new()));
            continue;
        };

        let mut records = Vec::new();
        for (number, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let record: Value = serde_json::from_str(line).map_err(|err| {
                Error::invalid(format!(
                    "{entry} line {} is not valid JSON: {err}",
                    number + 1
                ))
            })?;
            records.push(record);
        }
        data.push((collection.name.clone(), records));
    }

    Ok(Archive {
        manifest,
        schema,
        data,
    })
}

/// Read a JSON entry, or fail naming it.
fn read_json<R: Read + std::io::Seek, T: serde::de::DeserializeOwned>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<T> {
    let text = read_text(zip, name)?
        .ok_or_else(|| Error::invalid(format!("the archive has no {name}")))?;
    serde_json::from_str(&text)
        .map_err(|err| Error::invalid(format!("{name} is not valid JSON: {err}")))
}

/// Read one entry, or `None` when it is absent.
fn read_text<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Option<String>> {
    let mut entry = match zip.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(err) => {
            return Err(Error::invalid(format!(
                "cannot read {name} from the archive: {err}"
            )))
        }
    };

    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .map_err(|err| Error::invalid(format!("{name} is not readable text: {err}")))?;
    Ok(Some(text))
}

// ---------------------------------------------------------------------------
// Applying an archive
// ---------------------------------------------------------------------------

use crate::types::FieldType;

/// How an import applies its changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomicMode {
    /// Everything in one transaction: either the whole archive lands or none of it.
    Batch,
    /// One transaction per collection, so one bad collection costs only itself.
    PerCollection,
    /// No shared transaction: carry on past a failure and report each one.
    Continue,
}

impl AtomicMode {
    /// Parse the value the import form sends.
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "batch" => Ok(AtomicMode::Batch),
            "per_collection" | "perCollection" => Ok(AtomicMode::PerCollection),
            "false" | "none" | "continue" => Ok(AtomicMode::Continue),
            other => Err(Error::invalid(format!(
                "atomic must be `batch`, `per_collection` or `false`, got {other:?}"
            ))),
        }
    }

    /// The name it reports itself as.
    pub fn as_str(self) -> &'static str {
        match self {
            AtomicMode::Batch => "batch",
            AtomicMode::PerCollection => "per_collection",
            AtomicMode::Continue => "false",
        }
    }
}

/// What an import was asked to do.
#[derive(Debug, Clone, Copy)]
pub struct ImportOptions {
    /// How to apply it.
    pub atomic: AtomicMode,
    /// Whether a large import has been confirmed.
    pub confirm: bool,
    /// The record count above which confirmation is required.
    pub confirm_threshold: u64,
}

impl Default for ImportOptions {
    fn default() -> Self {
        // `batch` is the safest default: a failure leaves the database as it was
        // rather than half-restored.
        Self {
            atomic: AtomicMode::Batch,
            confirm: false,
            confirm_threshold: 10_000,
        }
    }
}

/// What an archive holds, computed before anything is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Preflight {
    /// The collections it would write, with their row counts.
    pub collections: Vec<ManifestCollection>,
    /// Collections it refuses to touch, with the reason.
    pub skipped: Vec<(String, String)>,
    /// Records in total.
    pub total_records: u64,
    /// Whether confirmation is needed before applying.
    pub requires_confirmation: bool,
}

/// One collection's result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportedCollection {
    /// Table name.
    pub name: String,
    /// `base`, `auth` or `view`.
    #[serde(rename = "type")]
    pub collection_type: Option<String>,
    /// Records written, which is `records_imported` on the wire.
    #[serde(rename = "records_imported")]
    pub records_imported: u64,
}

/// What an import did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    /// Collections written.
    pub imported: Vec<ImportedCollection>,
    /// Collections refused, with the reason.
    pub skipped: Vec<(String, String)>,
    /// Collections that failed, with the reason. Only ever non-empty under
    /// [`AtomicMode::Continue`], which is the point of that mode.
    pub errors: Vec<(String, String)>,
}

/// The result of asking to import.
#[derive(Debug, Clone)]
pub enum ImportOutcome {
    /// It was applied.
    Applied(Box<ImportReport>),
    /// It is large enough to need confirmation first.
    NeedsConfirmation(Box<Preflight>),
}

/// Work out what an import would do, without writing anything.
pub fn preflight(archive: &Archive, options: &ImportOptions) -> Preflight {
    let records_in = |name: &str| -> u64 {
        archive
            .data
            .iter()
            .find(|(collection, _)| collection == name)
            .map(|(_, records)| records.len() as u64)
            .unwrap_or(0)
    };

    let mut collections = Vec::new();
    let mut skipped = Vec::new();
    let mut total_records = 0;

    for (name, action) in &archive.plan(&[]).actions {
        match action {
            ImportAction::Skip(reason) => skipped.push((name.clone(), reason.clone())),
            _ => {
                let definition = archive
                    .schema
                    .collections
                    .iter()
                    .find(|collection| &collection.name == name);
                let records = records_in(name);
                total_records += records;
                collections.push(ManifestCollection {
                    name: name.clone(),
                    collection_type: definition.and_then(|c| c.collection_type.clone()),
                    records,
                });
            }
        }
    }

    Preflight {
        requires_confirmation: total_records > options.confirm_threshold && !options.confirm,
        collections,
        skipped,
        total_records,
    }
}

/// Apply an archive.
///
/// The two safety rules hold here regardless of mode, of what the file says, or of
/// what a client sends: a collection the archive omits is never touched, and a
/// system collection is never written.
pub async fn import(
    client: &mut Client,
    archive: &Archive,
    options: &ImportOptions,
) -> Result<ImportOutcome> {
    let existing: Vec<String> = collections::list(client)
        .await?
        .into_iter()
        .map(|collection| collection.name)
        .collect();
    let import_plan = archive.plan(&existing);

    let preflight = preflight(archive, options);
    if preflight.requires_confirmation {
        return Ok(ImportOutcome::NeedsConfirmation(Box::new(preflight)));
    }

    let mut report = ImportReport {
        imported: Vec::new(),
        skipped: preflight.skipped.clone(),
        errors: Vec::new(),
    };

    // `batch` runs the whole archive in one transaction, so borrow the client once
    // and let the transaction back it. The other modes drive their own.
    // Definitions first, outside the data phase: `collections::create` manages its
    // own transaction and cannot join an outer one, so a collection's table is not
    // created inside the batch transaction below. Stated rather than glossed,
    // because it means `batch` covers the records, not the table creation.
    for (name, action) in import_plan.writable() {
        if matches!(action, ImportAction::Create) {
            create_collection(client, archive, name).await?;
        }
    }

    match options.atomic {
        AtomicMode::Batch => {
            let transaction = client.transaction().await.map_err(db_error)?;
            for (name, _) in import_plan.writable() {
                apply_records(&transaction, archive, name).await?;
                report
                    .imported
                    .push(imported_of(archive, name, &ImportAction::Update));
            }
            transaction.commit().await.map_err(db_error)?;
        }
        AtomicMode::PerCollection | AtomicMode::Continue => {
            for (name, action) in import_plan.writable() {
                // `per_collection` commits one collection at a time so a failure in
                // one does not discard the others; `continue` is the same but also
                // carries on reporting rather than stopping at the first failure.
                let transaction = client.transaction().await.map_err(db_error)?;
                match apply_records(&transaction, archive, name).await {
                    Ok(()) => match transaction.commit().await {
                        Ok(()) => report.imported.push(imported_of(archive, name, action)),
                        Err(err) => report.errors.push((name.clone(), err.to_string())),
                    },
                    Err(err) => {
                        let _ = transaction.rollback().await;
                        report
                            .errors
                            .push((name.clone(), err.message().to_string()));
                        if options.atomic == AtomicMode::PerCollection {
                            // Stop at the first failure: the rest is reported as not
                            // attempted rather than silently skipped.
                            for (remaining, _) in import_plan.writable() {
                                if !report.imported.iter().any(|done| &done.name == remaining)
                                    && !report.errors.iter().any(|(failed, _)| failed == remaining)
                                {
                                    report.errors.push((
                                        remaining.clone(),
                                        "not attempted: an earlier collection failed".to_string(),
                                    ));
                                }
                            }
                            break;
                        }
                    }
                }
            }
        }
    }

    tracing::info!(
        imported = report.imported.len(),
        skipped = report.skipped.len(),
        errors = report.errors.len(),
        atomic = options.atomic.as_str(),
        "applied a backup archive"
    );

    Ok(ImportOutcome::Applied(Box::new(report)))
}

/// What one collection will report.
fn imported_of(archive: &Archive, name: &str, _action: &ImportAction) -> ImportedCollection {
    ImportedCollection {
        name: name.to_string(),
        collection_type: archive
            .schema
            .collections
            .iter()
            .find(|collection| collection.name == name)
            .and_then(|collection| collection.collection_type.clone()),
        records_imported: archive
            .data
            .iter()
            .find(|(collection, _)| collection == name)
            .map(|(_, records)| records.len() as u64)
            .unwrap_or(0),
    }
}

/// Create a collection from its archive definition.
///
/// Fields are mapped explicitly rather than by deserialising the archive's shape
/// into the data layer's, because the two spellings differ (`schema` versus
/// `fields`) and a silent mismatch would create a collection with no columns.
async fn create_collection(client: &mut Client, archive: &Archive, name: &str) -> Result<()> {
    let definition = archive
        .schema
        .collections
        .iter()
        .find(|collection| collection.name == name)
        .ok_or_else(|| Error::invalid(format!("the archive has no definition for {name:?}")))?;

    let mut fields = Vec::with_capacity(definition.schema.len());
    for field in &definition.schema {
        fields.push(field_to_create(field)?);
    }

    crate::collections::create(
        client,
        crate::collections::CreateCollection {
            name: name.to_string(),
            collection_type: parse_collection_type(definition.collection_type.as_deref())?,
            fields,
            rules: definition
                .rules
                .clone()
                .unwrap_or_else(|| serde_json::json!({})),
            options: definition
                .options
                .clone()
                .unwrap_or_else(|| serde_json::json!({})),
            adopt_existing_table: false,
        },
    )
    .await?;

    Ok(())
}

/// Write one collection's records.
async fn apply_records(
    runner: &dyn crate::runner::QueryRunner,
    archive: &Archive,
    name: &str,
) -> Result<()> {
    let Some((_, records)) = archive
        .data
        .iter()
        .find(|(collection, _)| collection == name)
    else {
        return Ok(());
    };

    for record in records {
        upsert(runner, name, record).await?;
    }

    Ok(())
}

/// Insert or update one record by `id`.
///
/// `on conflict (id) do update` is what makes an import idempotent and, more
/// importantly, makes it *additive*: a record the archive carries is updated, and a
/// record it does not is left alone, because nothing here deletes.
async fn upsert(
    runner: &dyn crate::runner::QueryRunner,
    table: &str,
    record: &Value,
) -> Result<()> {
    let Some(object) = record.as_object() else {
        return Err(Error::invalid(format!(
            "a record in {table} is not an object"
        )));
    };
    if !object.contains_key("id") {
        return Err(Error::invalid(format!(
            "a record in {table} has no id; an import upserts by id"
        )));
    }

    // Every column except `id` is updated from the incoming row, so a restore is a
    // faithful copy rather than a merge that keeps stale values.
    let columns: Vec<&String> = object
        .keys()
        .filter(|column| column.as_str() != "id")
        .collect();
    let assignments = columns
        .iter()
        .map(|column| {
            format!(
                "{quoted} = excluded.{quoted}",
                quoted = crate::ddl::quote_ident(column)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");

    let sql = format!(
        "insert into {table}          select * from jsonb_populate_record(null::{table}, $1::jsonb)          on conflict (\"id\") do {action}",
        table = crate::ddl::quote_ident(table),
        action = if assignments.is_empty() {
            // Nothing but an id: an insert that conflicts has nothing to change.
            "nothing".to_string()
        } else {
            format!("update set {assignments}")
        }
    );

    runner
        .run_execute(&sql, &[&record])
        .await
        .map_err(|err| Error::invalid(format!("cannot import a record into {table}: {err}")))?;
    Ok(())
}

/// Map an archive's field object onto the data layer's input.
fn field_to_create(value: &Value) -> Result<crate::collections::CreateField> {
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid("an archive field has no name"))?;
    let field_type = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid(format!("the field {name:?} has no type")))?;

    Ok(crate::collections::CreateField {
        name: name.to_string(),
        field_type: FieldType::parse(field_type)?,
        required: value
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        unique: value
            .get("unique")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        options: value
            .get("options")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    })
}

/// Map an archive's collection type onto this server's.
fn parse_collection_type(value: Option<&str>) -> Result<crate::types::CollectionType> {
    match value.unwrap_or("base") {
        "base" => Ok(crate::types::CollectionType::Base),
        "auth" => Ok(crate::types::CollectionType::Auth),
        "view" => Ok(crate::types::CollectionType::View),
        other => Err(Error::invalid(format!(
            "unknown collection type {other:?}; expected base, auth or view"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collection(name: &str) -> ArchiveCollection {
        ArchiveCollection {
            name: name.into(),
            collection_type: Some("base".into()),
            schema: Vec::new(),
            rules: None,
            options: None,
            records: Vec::new(),
        }
    }

    #[test]
    fn an_import_never_deletes_a_collection_the_archive_omits() {
        // The rule, stated as a test: the archive mentions `posts`, the database has
        // `posts` and `orders`, and `orders` must not appear in the plan at all.
        let archive = vec![collection("posts")];
        let existing = vec!["posts".to_string(), "orders".to_string()];
        let plan = plan(&archive, &existing);

        let touched: Vec<&str> = plan.actions.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            touched,
            vec!["posts"],
            "a collection the archive does not mention must not be planned at all"
        );
        assert_eq!(plan.actions[0].1, ImportAction::Update);
    }

    #[test]
    fn a_partial_archive_leaves_everything_else_alone() {
        // The failure this exists to prevent: a one-collection file must not wipe the
        // rest. Nothing that is absent is ever an action.
        let archive = vec![collection("posts")];
        let existing = (0..50)
            .map(|index| format!("table_{index}"))
            .chain(std::iter::once("posts".to_string()))
            .collect::<Vec<_>>();

        let plan = plan(&archive, &existing);
        assert_eq!(plan.actions.len(), 1, "{plan:?}");
    }

    #[test]
    fn a_system_collection_is_refused_whatever_else_the_archive_says() {
        let archive = vec![
            collection("_superusers"),
            collection("_collections"),
            collection("_settings"),
            collection("_lazypock_migrations"),
            collection("posts"),
        ];
        let plan = plan(&archive, &[]);

        assert_eq!(plan.skipped().count(), 4, "{plan:?}");
        for (name, action) in plan.skipped() {
            assert!(is_system_collection(name), "{name}");
            assert!(
                matches!(action, ImportAction::Skip(reason) if reason.contains("system")),
                "{name}: {action:?}"
            );
        }

        // The ordinary collection is still imported.
        let writable: Vec<&str> = plan.writable().map(|(name, _)| name.as_str()).collect();
        assert_eq!(writable, vec!["posts"]);
    }

    #[test]
    fn the_system_rule_matches_the_convention_reflection_uses() {
        // Both decisions key off the same prefix, so they cannot disagree about what
        // is lazypock's own.
        for name in [
            "_collections",
            "_fields",
            "_settings",
            "_crons",
            "_superusers",
            "_image_presets",
            "_file_outbox",
            "_rt_log",
        ] {
            assert!(is_system_collection(name), "{name}");
        }
        for name in ["posts", "orders", "user_profiles", "a_b_c"] {
            assert!(!is_system_collection(name), "{name}");
        }
    }

    #[test]
    fn an_existing_collection_is_updated_and_a_new_one_created() {
        let archive = vec![collection("posts"), collection("orders")];
        let plan = plan(&archive, &["posts".to_string()]);

        // `posts` is already there, so it is updated; `orders` is not, so it is
        // created. I had these the wrong way round first, which is exactly the
        // confusion this test exists to pin down.
        assert_eq!(plan.actions[0].1, ImportAction::Update);
        assert_eq!(plan.actions[1].1, ImportAction::Create);
        assert_eq!(plan.writable().count(), 2);
        assert!(!plan.is_empty());
    }

    #[test]
    fn an_archive_of_only_system_collections_writes_nothing() {
        let archive = vec![collection("_superusers"), collection("_settings")];
        let plan = plan(&archive, &[]);
        assert!(plan.is_empty(), "{plan:?}");
        assert_eq!(plan.skipped().count(), 2);
    }

    #[test]
    fn the_archive_format_accepts_both_spellings_of_fields() {
        // The published format uses `schema`; this server's wire format uses
        // `fields`. A file written by either has to load.
        let published: ArchiveSchema = serde_json::from_str(
            r#"{"collections":[{"name":"posts","type":"base","schema":[{"name":"title","type":"text"}]}]}"#,
        )
        .expect("the published spelling");
        assert_eq!(published.collections[0].schema.len(), 1);

        let server: ArchiveSchema = serde_json::from_str(
            r#"{"collections":[{"name":"posts","type":"base","fields":[{"name":"title","type":"text"}]}]}"#,
        )
        .expect("this server's spelling");
        assert_eq!(server.collections[0].schema.len(), 1);
        assert_eq!(published.collections[0].name, server.collections[0].name);
    }

    #[test]
    fn an_empty_fields_list_does_not_hide_the_schema() {
        // `fields` is preferred only when it actually carries something: an empty
        // list beside a populated `schema` is not a definition of zero columns.
        let parsed: ArchiveSchema = serde_json::from_str(
            r#"{"collections":[{"name":"posts","fields":[],"schema":[{"name":"title","type":"text"}]}]}"#,
        )
        .expect("parses");
        assert_eq!(parsed.collections[0].schema.len(), 1);
    }

    #[test]
    fn a_studio_export_carrying_both_spellings_still_imports() {
        // What the Studio's "Export Collections" button writes is the objects
        // `GET /api/collections` serves, and since this server serves the derived
        // `schema` beside `fields` those objects carry both spellings. They used to be
        // rejected with "duplicate field `schema`", so the exporter's own output could
        // not be imported. This is that file, in miniature.
        let exported = serde_json::json!([
            {
                "id": "users",
                "name": "users",
                "type": "auth",
                "system": false,
                "created": "2026-10-15T12:36:26Z",
                "updated": "2026-10-15T12:36:26Z",
                "fields": [{
                    "id": "users_email",
                    "collectionId": "users",
                    "name": "email",
                    "type": "email",
                    "required": true,
                    "unique": true,
                    "hidden": false,
                    "system": false,
                    "indexed": false,
                    "sort_order": 0,
                    "options": {}
                }],
                "schema": [{
                    "name": "email",
                    "type": "email",
                    "required": true,
                    "unique": true,
                    "indexed": false,
                    "default": null,
                    "options": {}
                }],
                "rules": {},
                "indexes": [],
                "viewQuery": "",
                "viewOrigin": "",
                "viewBuilder": null,
                "options": {}
            },
            {
                // The Studio exports the system collections too, and they must still be
                // skipped rather than written.
                "name": "_superusers",
                "type": "auth",
                "system": true,
                "fields": [{"name": "email", "type": "email"}],
                "schema": [{"name": "email", "type": "email"}]
            }
        ]);

        let archive = archive_from_payload(&exported).expect("the Studio's own export must import");
        assert_eq!(archive.schema.collections.len(), 2);

        let users = &archive.schema.collections[0];
        assert_eq!(users.name, "users");
        assert_eq!(users.collection_type.as_deref(), Some("auth"));
        assert_eq!(users.schema.len(), 1);
        // `fields` is what both spellings mean, and it is the richer of the two: the
        // derived `schema` beside it has no id, collectionId or sort_order.
        assert_eq!(users.schema[0]["id"], serde_json::json!("users_email"));

        let plan = archive.plan(&["users".to_string()]);
        assert_eq!(plan.actions[0].1, ImportAction::Update);
        assert_eq!(plan.skipped().count(), 1, "{plan:?}");
    }

    #[test]
    fn a_manifest_round_trips_and_identifies_itself() {
        let manifest = Manifest {
            format: ARCHIVE_FORMAT.into(),
            format_version: ARCHIVE_FORMAT_VERSION,
            lazypock_version: Some("0.1.0".into()),
            created_at: Some("2026-01-01T00:00:00Z".into()),
            collections: vec![ManifestCollection {
                name: "posts".into(),
                collection_type: Some("base".into()),
                records: 812_345,
            }],
            files: Some(ManifestFiles {
                total: 10,
                bytes: 1024,
                missing: 1,
            }),
            totals: Some(ManifestTotals {
                collections: 1,
                records: 812_345,
                files: 10,
            }),
        };

        let json = serde_json::to_string(&manifest).expect("serialise");
        let parsed: Manifest = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed.format, ARCHIVE_FORMAT);
        assert_eq!(parsed.format_version, ARCHIVE_FORMAT_VERSION);
        // The row count is in the manifest so progress needs no unpacking.
        assert_eq!(parsed.collections[0].records, 812_345);
        // A missing upload is recorded rather than fatal.
        assert_eq!(parsed.files.expect("files").missing, 1);
    }

    /// A small archive with one collection and two records.
    fn sample() -> Archive {
        Archive {
            manifest: Manifest {
                format: ARCHIVE_FORMAT.into(),
                format_version: ARCHIVE_FORMAT_VERSION,
                lazypock_version: Some("0.1.0".into()),
                created_at: Some("2026-01-01T00:00:00Z".into()),
                collections: vec![ManifestCollection {
                    name: "posts".into(),
                    collection_type: Some("base".into()),
                    records: 2,
                }],
                files: Some(ManifestFiles::default()),
                totals: Some(ManifestTotals {
                    collections: 1,
                    records: 2,
                    files: 0,
                }),
            },
            schema: ArchiveSchema {
                collections: vec![collection("posts")],
            },
            data: vec![(
                "posts".into(),
                vec![
                    serde_json::json!({"id": "p1", "title": "Hello"}),
                    serde_json::json!({"id": "p2", "title": "World"}),
                ],
            )],
        }
    }

    #[test]
    fn an_archive_round_trips_through_the_wire_format() {
        let archive = sample();
        let bytes = write(&archive).expect("write");
        let read_back = read(&bytes).expect("read");

        assert_eq!(read_back.manifest.format, ARCHIVE_FORMAT);
        assert_eq!(read_back.manifest.format_version, ARCHIVE_FORMAT_VERSION);
        assert_eq!(read_back.manifest.collections[0].records, 2);
        assert_eq!(
            read_back.manifest.totals.as_ref().expect("totals").records,
            2
        );
        assert_eq!(read_back.schema.collections.len(), 1);
        assert_eq!(read_back.schema.collections[0].name, "posts");
        assert_eq!(read_back.record_count(), 2);
        assert_eq!(
            read_back.data[0].1[0]["title"],
            serde_json::json!("Hello"),
            "record contents survive"
        );
    }

    #[test]
    fn the_manifest_is_the_first_entry_because_a_client_peeks_at_it() {
        // Studio reads the first local file header to show an import preview without
        // unpacking the archive, so nothing may be written before the manifest.
        let bytes = write(&sample()).expect("write");
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes.clone())).expect("a readable archive");
        assert_eq!(zip.by_index(0).expect("an entry").name(), MANIFEST_ENTRY);

        // And the header it peeks at must be self-describing: Studio reads the
        // compressed size out of the local file header, so a data descriptor (bit
        // 3 of the flags) or ZIP64 would make it give up quietly. Asserted on the
        // bytes, because "the library does not do that" is not a property.
        assert_eq!(
            &bytes[0..4],
            b"PK\x03\x04",
            "the first entry is a local header"
        );
        let flags = u16::from_le_bytes([bytes[6], bytes[7]]);
        assert_eq!(
            flags & 0x08,
            0,
            "bit 3 must be clear: sizes live in the local header, not a trailing descriptor"
        );
        let compressed = u32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]);
        assert_ne!(
            compressed, 0xffff_ffff,
            "ZIP64 would hide the size from a peeking client"
        );
        assert!(
            compressed > 0,
            "the manifest's compressed size is in the header"
        );
    }

    #[test]
    fn a_value_containing_a_newline_still_occupies_one_line() {
        // The property NDJSON depends on: a newline *inside* a value must be escaped,
        // or the record would split across two lines and a reader counting lines would
        // see records that do not exist. Asserting merely that the text holds no
        // newline proves nothing — it holds one per record by design, which is what my
        // first version of this test got wrong.
        let mut archive = sample();
        archive.data = vec![(
            "posts".into(),
            vec![serde_json::json!({
                "id": "p1",
                "body": "first\nsecond\ttabbed \"quoted\"",
            })],
        )];

        let bytes = write(&archive).expect("write");
        let read_back = read(&bytes).expect("read");
        assert_eq!(
            read_back.data[0].1.len(),
            1,
            "one record in, one record out"
        );
        assert_eq!(
            read_back.data[0].1[0]["body"],
            serde_json::json!("first\nsecond\ttabbed \"quoted\""),
            "the value survives intact"
        );
    }
    #[test]
    fn the_data_entries_are_one_record_per_line() {
        let bytes = write(&sample()).expect("write");
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("a readable archive");
        let mut entry = zip.by_name("data/posts.ndjson").expect("the data entry");
        let mut text = String::new();
        entry.read_to_string(&mut text).expect("read");

        assert_eq!(text.lines().count(), 2, "{text:?}");
        // Each line is independently valid, which is what makes `wc -l` and `tail`
        // work on a collection too large to hold in memory.
        for line in text.lines() {
            serde_json::from_str::<Value>(line).expect("each line is a record");
        }
    }

    #[test]
    fn an_empty_collection_writes_an_empty_entry_rather_than_none() {
        let mut archive = sample();
        archive.data = vec![("posts".into(), Vec::new())];
        archive.manifest.collections[0].records = 0;

        let bytes = write(&archive).expect("write");
        let read_back = read(&bytes).expect("read");
        assert_eq!(read_back.data[0].1.len(), 0);
        assert_eq!(read_back.record_count(), 0);
    }

    #[test]
    fn a_foreign_or_newer_archive_is_refused_rather_than_partly_applied() {
        // Something that is not an archive at all.
        let error = read(b"not a zip").expect_err("refused");
        assert!(error.message().contains("archive"), "{}", error.message());

        // A valid archive from a future format version: refused, because it may hold
        // things this server would silently drop.
        let mut archive = sample();
        archive.manifest.format_version = ARCHIVE_FORMAT_VERSION + 1;
        let bytes = write(&archive).expect("write");
        let error = read(&bytes).expect_err("refused");
        assert!(
            error.message().contains("newer") || error.message().contains("format version"),
            "{}",
            error.message()
        );

        // A zip that is not a lazypock archive at all.
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(MANIFEST_ENTRY, zip::write::SimpleFileOptions::default())
            .expect("start");
        writer
            .write_all(br#"{"format":"something-else","format_version":1}"#)
            .expect("write");
        let bytes = writer.finish().expect("finish").into_inner();
        let error = read(&bytes).expect_err("refused");
        assert!(
            error.message().contains("not a lazypock archive"),
            "{}",
            error.message()
        );
    }

    #[test]
    fn the_timestamp_is_formatted_as_rfc_3339() {
        assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_rfc3339(1_000_000_000), "2001-09-09T01:46:40Z");
        assert_eq!(format_rfc3339(1_700_000_000), "2023-11-14T22:13:20Z");
        assert!(now_rfc3339().ends_with('Z'));
    }

    #[test]
    fn a_manifest_without_the_optional_blocks_still_loads() {
        let parsed: Manifest =
            serde_json::from_str(r#"{"format":"lazypock-archive","format_version":1}"#)
                .expect("a minimal manifest");
        assert!(parsed.collections.is_empty());
        assert!(parsed.files.is_none());
    }
}
