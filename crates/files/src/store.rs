//! The media library: `_files` rows, their references and their deletion queue.
//!
//! Metadata only — this module never touches a storage backend. That ordering is
//! deliberate and runs through everything here: a row is written when the object is
//! stored, a row is removed when the file is gone from the library, and the object
//! itself is removed afterwards by [`crate::reaper`], which reads the queue the
//! `_files_deleted` trigger wrote. A row therefore never points at an object that
//! was deleted early, and a storage outage only delays cleanup.
//!
//! Timestamps come back as RFC 3339 strings formatted in SQL, which is the shape the
//! API serves and keeps this crate free of a date/time dependency.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use lazypock_core::error::{Error, Result};
use lazypock_db::pool::db_error;
use lazypock_db::query::compile::CompiledFilter;
use lazypock_db::runner::QueryRunner;

/// Where an upload came from.
///
/// An upload from a record's field arrives attached; one from the richtext editor or
/// from the library may never be attached at all, which is what the reaper's
/// reclaim rule keys off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// A record's file field.
    Field,
    /// The richtext editor.
    Editor,
    /// The media library.
    Library,
}

impl Origin {
    /// The name `_files.origin` stores.
    pub const fn as_str(self) -> &'static str {
        match self {
            Origin::Field => "field",
            Origin::Editor => "editor",
            Origin::Library => "library",
        }
    }

    /// Parse a stored or posted name. Empty means [`Origin::Field`], since that is
    /// what a caller who says nothing means.
    pub fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "field" => Ok(Origin::Field),
            "editor" => Ok(Origin::Editor),
            "library" => Ok(Origin::Library),
            other => Err(Error::invalid(format!(
                "unknown file origin {other:?}; expected field, editor or library"
            ))),
        }
    }
}

/// Whether an upload is attached to a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Stored, not attached to a record yet.
    Pending,
    /// Attached: the row names its collection, record and field.
    Ready,
}

impl Status {
    /// The name `_files.status` stores.
    pub const fn as_str(self) -> &'static str {
        match self {
            Status::Pending => "pending",
            Status::Ready => "ready",
        }
    }

    /// Parse a stored name.
    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "pending" => Ok(Status::Pending),
            "ready" => Ok(Status::Ready),
            other => Err(Error::internal(format!(
                "unknown file status {other:?} in _files"
            ))),
        }
    }
}

/// The columns every file query selects.
const COLUMNS: &str = "id, filename, extension, mime_type, size, storage_path, \
     storage_backend, collection_name, record_id, field_name, variants, status, origin, \
     original_name, width, height, checksum, uploaded_by, \
     to_char(attached_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') as attached_at, \
     to_char(created_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') as created_at, \
     to_char(updated_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') as updated_at";

/// One stored file, as `_files` holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileRow {
    /// The file's id: what a client stores in a file field and hands back.
    pub id: String,
    /// Name on disk and in URLs.
    pub filename: String,
    /// Extension without the dot.
    pub extension: String,
    /// Sniffed content type, never the caller's claim.
    pub mime_type: String,
    /// Size in bytes.
    pub size: i64,
    /// Where the object is, relative to the backend's root.
    pub storage_path: String,
    /// Which backend holds it: `local` or `s3`.
    pub storage_backend: String,
    /// Owning collection, or empty.
    pub collection_name: String,
    /// Owning record, or empty.
    pub record_id: String,
    /// Owning field, or empty.
    pub field_name: String,
    /// Generated variants: preset name => storage key.
    pub variants: Value,
    /// `pending` or `ready`.
    pub status: String,
    /// `field`, `editor` or `library`.
    pub origin: String,
    /// The name the client uploaded, kept for display.
    pub original_name: Option<String>,
    /// Pixel width, for an image.
    pub width: Option<i32>,
    /// Pixel height, for an image.
    pub height: Option<i32>,
    /// Content checksum, when one was computed.
    pub checksum: Option<String>,
    /// The uploader's id, or empty.
    pub uploaded_by: String,
    /// When it was attached to a record, RFC 3339.
    pub attached_at: Option<String>,
    /// When it was stored, RFC 3339.
    pub created_at: String,
    /// When the row last changed, RFC 3339.
    pub updated_at: String,
}

impl FileRow {
    /// Read a file from a row selected with [`COLUMNS`].
    pub fn from_row(row: &tokio_postgres::Row) -> Result<Self> {
        Ok(Self {
            id: row.get("id"),
            filename: row.get("filename"),
            extension: row.get("extension"),
            mime_type: row.get("mime_type"),
            size: row.get("size"),
            storage_path: row.get("storage_path"),
            storage_backend: row.get("storage_backend"),
            collection_name: row.get("collection_name"),
            record_id: row.get("record_id"),
            field_name: row.get("field_name"),
            variants: row.get("variants"),
            status: row.get("status"),
            origin: row.get("origin"),
            original_name: row.get("original_name"),
            width: row.get("width"),
            height: row.get("height"),
            checksum: row.get("checksum"),
            uploaded_by: row.get("uploaded_by"),
            attached_at: row.get("attached_at"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }

    /// Whether the file is attached to a record.
    pub fn is_attached(&self) -> bool {
        !self.collection_name.is_empty() && !self.record_id.is_empty()
    }

    /// The variants as a name => key map, tolerating a malformed column.
    pub fn variant_keys(&self) -> Vec<String> {
        self.variants
            .as_object()
            .map(|map| {
                map.values()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// What to insert for a newly stored object.
#[derive(Debug, Clone)]
pub struct NewFile {
    /// Name on disk and in URLs.
    pub filename: String,
    /// Extension without the dot.
    pub extension: String,
    /// Sniffed content type.
    pub mime_type: String,
    /// Size in bytes.
    pub size: i64,
    /// Where the object is.
    pub storage_path: String,
    /// Which backend holds it.
    pub storage_backend: String,
    /// Owning collection, or empty.
    pub collection_name: String,
    /// Owning record, or empty.
    pub record_id: String,
    /// Owning field, or empty.
    pub field_name: String,
    /// Where the upload came from.
    pub origin: Origin,
    /// The uploader's id, or empty.
    pub uploaded_by: String,
    /// The name the client uploaded.
    pub original_name: Option<String>,
    /// Pixel width, for an image.
    pub width: Option<i32>,
    /// Pixel height, for an image.
    pub height: Option<i32>,
    /// Content checksum, when computed.
    pub checksum: Option<String>,
}

impl NewFile {
    /// A file that is not attached to any record yet.
    pub fn unattached(filename: String, storage_path: String, backend: String) -> Self {
        Self {
            filename,
            extension: String::new(),
            mime_type: "application/octet-stream".to_string(),
            size: 0,
            storage_path,
            storage_backend: backend,
            collection_name: String::new(),
            record_id: String::new(),
            field_name: String::new(),
            origin: Origin::Field,
            uploaded_by: String::new(),
            original_name: None,
            width: None,
            height: None,
            checksum: None,
        }
    }

    /// Whether this upload arrives attached to a record.
    pub fn is_attached(&self) -> bool {
        !self.collection_name.is_empty() && !self.record_id.is_empty()
    }
}

/// Insert a row for a stored object, returning it as the table now holds it.
pub async fn insert(runner: &dyn QueryRunner, new: &NewFile) -> Result<FileRow> {
    let id = lazypock_core::id::new_id();
    let status = if new.is_attached() {
        Status::Ready
    } else {
        Status::Pending
    };

    runner
        .run_execute(
            "insert into _files (id, filename, extension, mime_type, size, storage_path, \
             storage_backend, collection_name, record_id, field_name, origin, status, \
             uploaded_by, original_name, width, height, checksum, attached_at) \
             values ($1::text, $2::text, $3::text, $4::text, $5::text::bigint, $6::text, \
             $7::text, $8::text, $9::text, $10::text, $11::text, $12::text, $13::text, \
             $14::text, $15::text::integer, $16::text::integer, $17::text, \
             case when $9::text = '' then null else now() end)",
            &[
                &id,
                &new.filename,
                &new.extension,
                &new.mime_type,
                &new.size.to_string(),
                &new.storage_path,
                &new.storage_backend,
                &new.collection_name,
                &new.record_id,
                &new.field_name,
                &new.origin.as_str().to_string(),
                &status.as_str().to_string(),
                &new.uploaded_by,
                &new.original_name,
                &new.width.map(|value| value.to_string()),
                &new.height.map(|value| value.to_string()),
                &new.checksum,
            ],
        )
        .await
        .map_err(db_error)?;

    get(runner, &id)
        .await?
        .ok_or_else(|| Error::internal("the file row vanished immediately after insert"))
}

/// One file, by id.
pub async fn get(runner: &dyn QueryRunner, id: &str) -> Result<Option<FileRow>> {
    let sql = format!("select {COLUMNS} from _files where id = $1::text");
    let row = runner
        .run_query_opt(&sql, &[&id.to_string()])
        .await
        .map_err(db_error)?;
    row.map(|row| FileRow::from_row(&row)).transpose()
}

/// What to filter a listing by.
#[derive(Debug, Clone, Default)]
pub struct ListQuery {
    /// One-based page.
    pub page: i64,
    /// Rows per page.
    pub per_page: i64,
    /// Only files of this collection.
    pub collection: Option<String>,
    /// Only files of this record.
    pub record: Option<String>,
    /// Only files in this field.
    pub field: Option<String>,
    /// Only this content type, or this prefix (`image/`).
    pub mime: Option<String>,
    /// Only names containing this text.
    pub search: Option<String>,
    /// Only files with this origin.
    pub origin: Option<Origin>,
    /// Only files with this status.
    pub status: Option<Status>,
    /// Only files uploaded by this identity.
    pub uploaded_by: Option<String>,
    /// An extra predicate the rows must match, from `files.rules.listRule`.
    ///
    /// Compiled by [`crate::rules`] against the caller's identity; `None` means the
    /// listing is not restricted by a rule (a public rule, or a superuser).
    pub restriction: Option<CompiledFilter>,
}

/// The rows a filter matches: `(where clauses, parameters)`.
///
/// Split out so the clause/parameter pairing can be asserted without a database —
/// an off-by-one in that pairing is exactly the kind of bug that would silently
/// filter on the wrong column.
fn filters(query: &ListQuery) -> (Vec<String>, Vec<String>) {
    let mut clauses: Vec<String> = Vec::new();
    let mut parameters: Vec<String> = Vec::new();

    let add =
        |clauses: &mut Vec<String>, parameters: &mut Vec<String>, clause: &str, value: String| {
            parameters.push(value);
            clauses.push(format!("{clause} ${}::text", parameters.len()));
        };

    if let Some(collection) = &query.collection {
        add(
            &mut clauses,
            &mut parameters,
            "collection_name =",
            collection.clone(),
        );
    }
    if let Some(record) = &query.record {
        add(&mut clauses, &mut parameters, "record_id =", record.clone());
    }
    if let Some(field) = &query.field {
        add(&mut clauses, &mut parameters, "field_name =", field.clone());
    }
    if let Some(mime) = &query.mime {
        // A prefix, so `image/` means every image type, which is what the Studio's
        // "images only" toggle sends.
        add(
            &mut clauses,
            &mut parameters,
            "mime_type like",
            format!("{mime}%"),
        );
    }
    if let Some(search) = &query.search {
        add(
            &mut clauses,
            &mut parameters,
            "filename ilike",
            format!("%{search}%"),
        );
    }
    if let Some(origin) = query.origin {
        add(
            &mut clauses,
            &mut parameters,
            "origin =",
            origin.as_str().to_string(),
        );
    }
    if let Some(status) = query.status {
        add(
            &mut clauses,
            &mut parameters,
            "status =",
            status.as_str().to_string(),
        );
    }
    if let Some(uploaded_by) = &query.uploaded_by {
        add(
            &mut clauses,
            &mut parameters,
            "uploaded_by =",
            uploaded_by.clone(),
        );
    }

    (clauses, parameters)
}

/// One page of the library, newest first, with the total the filter matches.
pub async fn list(runner: &dyn QueryRunner, query: &ListQuery) -> Result<(Vec<FileRow>, i64)> {
    let (mut clauses, mut parameters) = filters(query);

    // The rule's predicate is compiled on its own, so its placeholders start at $1 and
    // have to move past the filters' parameters. Getting that wrong would compare a
    // rule against the wrong value — hence the shift helper and its tests.
    if let Some(restriction) = &query.restriction {
        let offset = parameters.len();
        clauses.push(format!(
            "({})",
            crate::rules::shift_placeholders(&restriction.sql, offset)
        ));
        parameters.extend(restriction.params.iter().cloned());
    }

    let where_clause = if clauses.is_empty() {
        String::new()
    } else {
        format!("where {}", clauses.join(" and "))
    };

    let filter_params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = parameters
        .iter()
        .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect();

    let count_sql = format!("select count(*) from _files {where_clause}");
    let total: i64 = runner
        .run_query_one(&count_sql, &filter_params)
        .await
        .map_err(db_error)?
        .get(0);

    let per_page = query.per_page.clamp(1, 200);
    let offset = (query.page.max(1) - 1) * per_page;
    let mut page_parameters = parameters.clone();
    page_parameters.push(per_page.to_string());
    page_parameters.push(offset.to_string());

    let page_sql = format!(
        "select {COLUMNS} from _files {where_clause} \
         order by created_at desc, id desc limit ${}::text::bigint offset ${}::text::bigint",
        page_parameters.len() - 1,
        page_parameters.len()
    );
    let page_params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = page_parameters
        .iter()
        .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect();

    let rows = runner
        .run_query(&page_sql, &page_params)
        .await
        .map_err(db_error)?;

    let mut files = Vec::with_capacity(rows.len());
    for row in &rows {
        files.push(FileRow::from_row(row)?);
    }

    Ok((files, total))
}

/// Attach an upload to a record, which is what takes it out of the reaper's reach.
pub async fn attach(
    runner: &dyn QueryRunner,
    id: &str,
    collection: &str,
    record: &str,
    field: &str,
) -> Result<Option<FileRow>> {
    let updated = runner
        .run_execute(
            "update _files set collection_name = $2::text, record_id = $3::text, \
             field_name = $4::text, status = 'ready', attached_at = now(), updated_at = now() \
             where id = $1::text",
            &[
                &id.to_string(),
                &collection.to_string(),
                &record.to_string(),
                &field.to_string(),
            ],
        )
        .await
        .map_err(db_error)?;

    if updated == 0 {
        return Ok(None);
    }
    get(runner, id).await
}

/// Record the generated variants for a file.
pub async fn set_variants(runner: &dyn QueryRunner, id: &str, variants: &Value) -> Result<()> {
    // The `Value` is sent as a parameter rather than as a string: a `text` parameter for
    // a `jsonb` placeholder is not an input Postgres can parse ("error serializing
    // parameter", which reaches the caller as a bare 500).
    runner
        .run_execute(
            "update _files set variants = $2, updated_at = now() where id = $1::text",
            &[&id.to_string(), variants],
        )
        .await
        .map_err(db_error)?;
    Ok(())
}

/// Delete one row, returning it.
///
/// The object itself is queued by the `_files_deleted` trigger, in this transaction,
/// so the caller does not have to remember to ask for it.
pub async fn delete(runner: &dyn QueryRunner, id: &str) -> Result<Option<FileRow>> {
    let file = get(runner, id).await?;
    if file.is_none() {
        return Ok(None);
    }

    runner
        .run_execute("delete from _files where id = $1::text", &[&id.to_string()])
        .await
        .map_err(db_error)?;
    Ok(file)
}

/// Delete every file a record owns, and return their ids.
///
/// This is the call every record delete makes. The trigger queues each object, so
/// the storage catch-up happens on the reaper's next pass.
pub async fn delete_for_record(
    runner: &dyn QueryRunner,
    collection: &str,
    record: &str,
) -> Result<Vec<String>> {
    let rows = runner
        .run_query(
            "delete from _files where collection_name = $1::text and record_id = $2::text \
             returning id",
            &[&collection.to_string(), &record.to_string()],
        )
        .await
        .map_err(db_error)?;

    Ok(rows.iter().map(|row| row.get(0)).collect())
}

/// Delete every file a dropped collection owned, and return their ids.
pub async fn delete_for_collection(
    runner: &dyn QueryRunner,
    collection: &str,
) -> Result<Vec<String>> {
    let rows = runner
        .run_query(
            "delete from _files where collection_name = $1::text returning id",
            &[&collection.to_string()],
        )
        .await
        .map_err(db_error)?;

    Ok(rows.iter().map(|row| row.get(0)).collect())
}

/// Uploads that were never attached and are older than `ttl_seconds`.
///
/// The editor and the media library may upload something the user then abandons, and
/// those are what this reclaims. An upload that arrived through a record's field is
/// **not** a candidate even while it is pending: the user meant that file to be in a
/// record, and a bug in attaching it is not a reason to delete their upload.
pub async fn unattached_older_than(
    runner: &dyn QueryRunner,
    ttl_seconds: i64,
    limit: i64,
) -> Result<Vec<FileRow>> {
    let sql = format!(
        "select {COLUMNS} from _files \
         where status = 'pending' and attached_at is null and origin <> 'field' \
           and created_at < now() - make_interval(secs => $1::text::bigint) \
         order by created_at limit $2::text::bigint"
    );
    let rows = runner
        .run_query(&sql, &[&ttl_seconds.to_string(), &limit.to_string()])
        .await
        .map_err(db_error)?;

    let mut files = Vec::with_capacity(rows.len());
    for row in &rows {
        files.push(FileRow::from_row(row)?);
    }
    Ok(files)
}

/// Delete the given files, returning the ids that were there.
///
/// A record delete uses this for the files its values name, so a row that never got
/// attached to the record still goes when the record does. The trigger queues each
/// object, exactly as for every other deletion.
pub async fn delete_for_ids(runner: &dyn QueryRunner, ids: &[String]) -> Result<Vec<String>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let rows = runner
        .run_query(
            "delete from _files where id = any($1::text[]) returning id",
            &[&ids.to_vec()],
        )
        .await
        .map_err(db_error)?;

    Ok(rows.iter().map(|row| row.get(0)).collect())
}

/// What a file id found in a record is doing there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileLink {
    /// A `file` field's value: the file belongs to this record and this field.
    Owner,
    /// An id inside an editor field's markdown: the record references the file, which
    /// may belong to no record at all (an editor upload is stored before it is used).
    Reference,
}

/// One file a record points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundFile {
    /// The file's id.
    pub id: String,
    /// The field that names it.
    pub field: String,
    /// Whether the field owns the file or merely references it.
    pub link: FileLink,
}

/// Every file a record's values point at.
///
/// A `file` field holds the ids outright — one, or a list when the field allows
/// several — and **owns** them. An `editor` field holds markdown, so its ids are
/// whatever `/api/files/<id>` URLs it contains, which is the shape this server hands
/// out; those are references, because an editor upload exists before it is used and
/// two records may embed it.
///
/// An id that no longer exists simply matches no row when the caller deletes, so this
/// is a pure parse with no database round trip.
pub fn file_links_in(
    collection: &lazypock_db::collections::Collection,
    record: &Value,
) -> Vec<FoundFile> {
    use lazypock_db::types::FieldType;

    let Some(object) = record.as_object() else {
        return Vec::new();
    };

    let mut found: Vec<FoundFile> = Vec::new();
    let mut push = |id: String, field: &str, link: FileLink| {
        if !id.is_empty() {
            found.push(FoundFile {
                id,
                field: field.to_string(),
                link,
            });
        }
    };

    for field in &collection.fields {
        let Some(value) = object.get(&field.name) else {
            continue;
        };

        match field.field_type {
            FieldType::File => match value {
                Value::String(id) => push(id.clone(), &field.name, FileLink::Owner),
                Value::Array(values) => {
                    for value in values {
                        if let Some(id) = value.as_str() {
                            push(id.to_string(), &field.name, FileLink::Owner);
                        }
                    }
                }
                _ => {}
            },
            FieldType::Editor => {
                if let Some(text) = value.as_str() {
                    for id in ids_in_text(text) {
                        push(id, &field.name, FileLink::Reference);
                    }
                }
            }
            _ => {}
        }
    }

    // The same file can be named twice (a text embeds it, a field repeats it), and one
    // entry is all a caller can act on.
    found.sort_by(|left, right| {
        (&left.id, &left.field, left.link as u8).cmp(&(&right.id, &right.field, right.link as u8))
    });
    found.dedup();
    found
}

/// The file ids embedded in a value as `/api/files/<id>` URLs.
///
/// Hand-rolled rather than a regular expression: the id is a run of alphanumeric
/// characters and the prefix is a constant, so a scan is simpler and needs no
/// dependency.
fn ids_in_text(text: &str) -> Vec<String> {
    const PREFIX: &str = "/api/files/";

    let mut ids = Vec::new();
    let mut rest = text;
    while let Some(position) = rest.find(PREFIX) {
        let after = &rest[position + PREFIX.len()..];
        let id: String = after
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric())
            .collect();
        if !id.is_empty() {
            ids.push(id);
        }
        rest = after;
    }
    ids
}

/// One record a file is referenced by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRef {
    /// The collection holding the reference.
    pub collection: String,
    /// The record holding it.
    pub record_id: String,
    /// The field holding it.
    pub field: String,
}

/// Replace a record's references with the given `(file id, field)` pairs.
///
/// Called on record create and update. The set is replaced rather than merged: a
/// file removed from the editor's text must stop counting as a reference, or the
/// delete guard would refuse a delete that is in fact safe.
pub async fn sync_refs(
    runner: &dyn QueryRunner,
    collection: &str,
    record: &str,
    refs: &[(String, String)],
) -> Result<()> {
    runner
        .run_execute(
            "delete from _file_refs where collection = $1::text and record_id = $2::text \
             and (file_id, field) not in (select * from unnest($3::text[], $4::text[]))",
            &[
                &collection.to_string(),
                &record.to_string(),
                &refs.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
                &refs
                    .iter()
                    .map(|(_, field)| field.clone())
                    .collect::<Vec<_>>(),
            ],
        )
        .await
        .map_err(db_error)?;

    for (file_id, field) in refs {
        runner
            .run_execute(
                "insert into _file_refs (file_id, collection, record_id, field) \
                 values ($1::text, $2::text, $3::text, $4::text) on conflict do nothing",
                &[file_id, &collection.to_string(), &record.to_string(), field],
            )
            .await
            .map_err(db_error)?;
    }

    Ok(())
}

/// Drop a record's references, for when the record goes.
pub async fn delete_refs_for_record(
    runner: &dyn QueryRunner,
    collection: &str,
    record: &str,
) -> Result<()> {
    runner
        .run_execute(
            "delete from _file_refs where collection = $1::text and record_id = $2::text",
            &[&collection.to_string(), &record.to_string()],
        )
        .await
        .map_err(db_error)?;
    Ok(())
}

/// The records that reference a file, which is what a delete guard reports.
pub async fn refs_for_file(runner: &dyn QueryRunner, file_id: &str) -> Result<Vec<FileRef>> {
    let rows = runner
        .run_query(
            "select collection, record_id, field from _file_refs where file_id = $1::text \
             order by collection, record_id, field",
            &[&file_id.to_string()],
        )
        .await
        .map_err(db_error)?;

    Ok(rows
        .iter()
        .map(|row| FileRef {
            collection: row.get("collection"),
            record_id: row.get("record_id"),
            field: row.get("field"),
        })
        .collect())
}

/// Drop the references to files that no longer exist.
///
/// The `_file_refs.file_id` column has no foreign key — a reference may be recorded
/// before the row it points at is committed — so the cleanup is explicit.
pub async fn prune_refs(runner: &dyn QueryRunner) -> Result<u64> {
    runner
        .run_execute(
            "delete from _file_refs r where not exists (select 1 from _files f where f.id = r.file_id)",
            &[],
        )
        .await
        .map_err(db_error)
}

/// One object waiting to be removed from a backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingDeletion {
    /// Queue row.
    pub id: i64,
    /// The file the object belonged to.
    pub file_id: String,
    /// Which backend holds it.
    pub backend: String,
    /// The source object's key.
    pub storage_path: String,
    /// Its variants: preset name => key.
    pub variants: Value,
    /// Attempts so far.
    pub attempts: i32,
}

impl PendingDeletion {
    /// Every object this entry has to remove: the source, then its variants.
    pub fn keys(&self) -> Vec<String> {
        let mut keys = vec![self.storage_path.clone()];
        if let Some(map) = self.variants.as_object() {
            for value in map.values() {
                if let Some(key) = value.as_str() {
                    if key != self.storage_path {
                        keys.push(key.to_string());
                    }
                }
            }
        }
        keys
    }
}

/// The deletions that are due, oldest attempt first.
pub async fn due_deletions(runner: &dyn QueryRunner, limit: i64) -> Result<Vec<PendingDeletion>> {
    let rows = runner
        .run_query(
            "select id, file_id, backend, storage_path, variants, attempts from _file_deletions \
             where next_try_at <= now() order by next_try_at limit $1::text::bigint",
            &[&limit.to_string()],
        )
        .await
        .map_err(db_error)?;

    Ok(rows
        .iter()
        .map(|row| PendingDeletion {
            id: row.get("id"),
            file_id: row.get("file_id"),
            backend: row.get("backend"),
            storage_path: row.get("storage_path"),
            variants: row.get("variants"),
            attempts: row.get("attempts"),
        })
        .collect())
}

/// Forget a deletion whose objects are gone.
pub async fn finish_deletion(runner: &dyn QueryRunner, id: i64) -> Result<()> {
    runner
        .run_execute(
            "delete from _file_deletions where id = $1::text::bigint",
            &[&id.to_string()],
        )
        .await
        .map_err(db_error)?;
    Ok(())
}

/// Reschedule a deletion that could not be carried out.
pub async fn defer_deletion(
    runner: &dyn QueryRunner,
    id: i64,
    error: &str,
    max_backoff_seconds: i64,
) -> Result<()> {
    runner
        .run_execute(
            "update _file_deletions set attempts = attempts + 1, next_try_at = now() + \
             make_interval(secs => least($3::text::bigint, 30 * power(2, attempts))) \
             where id = $1::text::bigint and $2::text <> ''",
            &[
                &id.to_string(),
                &error.to_string(),
                &max_backoff_seconds.to_string(),
            ],
        )
        .await
        .map_err(db_error)?;
    Ok(())
}

/// How many deletions are waiting, and how old the oldest is.
pub async fn pending_deletions(runner: &dyn QueryRunner) -> Result<(i64, i64)> {
    let row = runner
        .run_query_one(
            "select count(*)::bigint as pending, \
             coalesce(extract(epoch from (now() - min(created_at)))::bigint, 0) as oldest_seconds \
             from _file_deletions",
            &[],
        )
        .await
        .map_err(db_error)?;
    Ok((row.get("pending"), row.get("oldest_seconds")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_round_trip_and_default_to_a_field() {
        for origin in [Origin::Field, Origin::Editor, Origin::Library] {
            assert_eq!(Origin::parse(origin.as_str()).expect("parse"), origin);
        }
        assert_eq!(Origin::parse("").expect("default"), Origin::Field);
        assert_eq!(
            Origin::parse(" LIBRARY ").expect("trimmed"),
            Origin::Library
        );
        // Refused rather than silently treated as a field: an origin decides whether
        // the reaper may reclaim an upload.
        assert!(Origin::parse("somewhere").is_err());
    }

    #[test]
    fn statuses_round_trip() {
        assert_eq!(Status::parse("pending").expect("pending"), Status::Pending);
        assert_eq!(Status::parse("ready").expect("ready"), Status::Ready);
        assert!(Status::parse("half").is_err());
    }

    #[test]
    fn filters_pair_every_clause_with_its_parameter() {
        let query = ListQuery {
            page: 1,
            per_page: 10,
            collection: Some("posts".into()),
            record: None,
            field: Some("cover".into()),
            mime: Some("image/".into()),
            search: Some("holiday".into()),
            origin: Some(Origin::Editor),
            status: Some(Status::Pending),
            uploaded_by: Some("u1".into()),
            restriction: None,
        };

        let (clauses, parameters) = filters(&query);
        assert_eq!(
            clauses,
            vec![
                "collection_name = $1::text",
                "field_name = $2::text",
                "mime_type like $3::text",
                "filename ilike $4::text",
                "origin = $5::text",
                "status = $6::text",
                "uploaded_by = $7::text",
            ]
        );
        assert_eq!(
            parameters,
            vec![
                "posts".to_string(),
                "cover".to_string(),
                "image/%".to_string(),
                "%holiday%".to_string(),
                "editor".to_string(),
                "pending".to_string(),
                "u1".to_string(),
            ]
        );
    }

    #[test]
    fn an_empty_filter_is_an_empty_where_clause() {
        let (clauses, parameters) = filters(&ListQuery::default());
        assert!(clauses.is_empty());
        assert!(parameters.is_empty());
    }

    #[test]
    fn a_deletion_removes_the_source_and_every_variant_once() {
        let pending = PendingDeletion {
            id: 1,
            file_id: "f1".into(),
            backend: "local".into(),
            storage_path: "posts/p1/abc_photo.png".into(),
            variants: serde_json::json!({
                "thumb": "posts/p1/thumbs/abc_photo.png/thumb_webp",
                // A variant that happens to name the source must not be deleted twice.
                "content": "posts/p1/abc_photo.png",
            }),
            attempts: 0,
        };

        assert_eq!(
            pending.keys(),
            vec![
                "posts/p1/abc_photo.png".to_string(),
                "posts/p1/thumbs/abc_photo.png/thumb_webp".to_string(),
            ]
        );
    }

    #[test]
    fn a_file_field_names_its_ids_and_an_editor_field_names_the_ids_in_its_text() {
        use lazypock_db::collections::{Collection, Field};
        use lazypock_db::types::{CollectionType, FieldType};

        let field = |name: &str, field_type: FieldType| Field {
            id: format!("f_{name}"),
            collection_id: "posts".into(),
            name: name.into(),
            field_type,
            required: false,
            is_unique: false,
            options: serde_json::json!({}),
            position: 0,
        };
        let collection = Collection {
            id: "posts".into(),
            name: "posts".into(),
            collection_type: CollectionType::Base,
            fields: vec![
                field("cover", FieldType::File),
                field("gallery", FieldType::File),
                field("body", FieldType::Editor),
                field("title", FieldType::Text),
            ],
            rules: serde_json::json!({}),
            options: serde_json::json!({}),
            created: std::time::SystemTime::UNIX_EPOCH,
            updated: std::time::SystemTime::UNIX_EPOCH,
        };

        let record = serde_json::json!({
            "cover": "AbcDef123456789",
            "gallery": ["SecondId1234567", ""] ,
            "body": "<p>look <img src=\"/api/files/EditorId1234567?thumb=sm\"></p>\
                     <p>and <a href=\"/api/files/NestedId123456\">again</a></p>",
            "title": "not a file"
        });

        let found = file_links_in(&collection, &record);
        assert_eq!(
            found,
            vec![
                FoundFile {
                    id: "AbcDef123456789".into(),
                    field: "cover".into(),
                    link: FileLink::Owner,
                },
                FoundFile {
                    id: "EditorId1234567".into(),
                    field: "body".into(),
                    link: FileLink::Reference,
                },
                FoundFile {
                    id: "NestedId123456".into(),
                    field: "body".into(),
                    link: FileLink::Reference,
                },
                FoundFile {
                    id: "SecondId1234567".into(),
                    field: "gallery".into(),
                    link: FileLink::Owner,
                },
            ]
        );
    }

    #[test]
    fn the_same_file_named_twice_is_one_reference() {
        use lazypock_db::collections::{Collection, Field};
        use lazypock_db::types::{CollectionType, FieldType};

        let collection = Collection {
            id: "posts".into(),
            name: "posts".into(),
            collection_type: CollectionType::Base,
            fields: vec![Field {
                id: "f_body".into(),
                collection_id: "posts".into(),
                name: "body".into(),
                field_type: FieldType::Editor,
                required: false,
                is_unique: false,
                options: serde_json::json!({}),
                position: 0,
            }],
            rules: serde_json::json!({}),
            options: serde_json::json!({}),
            created: std::time::SystemTime::UNIX_EPOCH,
            updated: std::time::SystemTime::UNIX_EPOCH,
        };

        let record = serde_json::json!({
            "body": "<img src=\"/api/files/SameId123456789\"><img src=\"/api/files/SameId123456789\">"
        });

        assert_eq!(
            file_links_in(&collection, &record),
            vec![FoundFile {
                id: "SameId123456789".into(),
                field: "body".into(),
                link: FileLink::Reference,
            }]
        );
    }

    #[test]
    fn ids_in_text_ignores_a_prefix_with_no_id_after_it() {
        assert!(ids_in_text("nothing here").is_empty());
        assert!(ids_in_text("/api/files/").is_empty());
        assert!(ids_in_text("/api/files/?thumb=sm").is_empty());
        // Two URLs, one of them with a query and one without: the id ends at the
        // first character that cannot be part of one.
        assert_eq!(
            ids_in_text("see /api/files/Abc123/scale/thumb and /api/files/Id9?thumb=sm"),
            vec!["Abc123".to_string(), "Id9".to_string()]
        );
        // A path that is not this server's file route is not a reference.
        assert_eq!(
            ids_in_text("/other/files/Id9 and /api/files/Abc123"),
            vec!["Abc123".to_string()]
        );
    }
}
