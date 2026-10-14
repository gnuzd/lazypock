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
/// `fields`; a file written by either is valid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveCollection {
    /// Table name.
    pub name: String,
    /// `base`, `auth` or `view`.
    #[serde(default, rename = "type")]
    pub collection_type: Option<String>,
    /// Field definitions, under either spelling.
    #[serde(default, alias = "fields")]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

    #[test]
    fn a_manifest_without_the_optional_blocks_still_loads() {
        let parsed: Manifest =
            serde_json::from_str(r#"{"format":"lazypock-archive","format_version":1}"#)
                .expect("a minimal manifest");
        assert!(parsed.collections.is_empty());
        assert!(parsed.files.is_none());
    }
}
