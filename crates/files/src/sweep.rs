//! Reclaiming stored objects that nothing refers to any more.
//!
//! The outbox handles the normal case: the transaction that removes a reference
//! queues the object, and the object goes away once that commits. The sweeper
//! handles everything the outbox cannot see — an object left by a crash between
//! commit and queue, a row deleted with SQL directly, a preset whose cached
//! thumbnails were never queued.
//!
//! It is deliberately **conservative**: an object is only reclaimed when the
//! thing that owns it is *provably* gone. A file whose record still exists is kept
//! even if some field stopped pointing at it, because nothing here tracks which
//! field referenced which file. Deleting on suspicion would destroy live data, and
//! the cost of keeping an unreferenced object is disk space.

use std::collections::{HashMap, HashSet};

use lazypock_core::error::Result;
use lazypock_db::pool::db_error;
use lazypock_db::runner::QueryRunner;
use lazypock_db::types::validate_identifier;

use crate::outbox;
use crate::storage::Storage;
use crate::thumbnail::OutputFormat;

/// How many orphan keys a report lists before it stops collecting examples.
const SAMPLE: usize = 50;

/// A key whose final segment is this suffix is an in-flight write, not an object.
const IN_FLIGHT_SUFFIX: &str = ".part";

/// What owns a stored object, judged from its key alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectOwner {
    /// A source file belonging to a record.
    Source {
        /// The collection the record lives in.
        collection: String,
        /// The record the file belongs to.
        record: String,
    },
    /// A generated thumbnail of a source file.
    Thumbnail {
        /// The collection the record lives in.
        collection: String,
        /// The record the source belongs to.
        record: String,
        /// The source file's key.
        source: String,
        /// The preset the thumbnail was generated from.
        preset: String,
    },
    /// Not a key this sweeper can attribute, so it is left alone.
    Unrecognised,
}

/// Judge an object from its key.
///
/// The layout is `{collection}/{record}/{filename}` for a source file and
/// `{collection}/{record}/thumbs/{filename}/{preset}_{format}` for a thumbnail.
pub fn classify(key: &str) -> ObjectOwner {
    // A temporary file from a write that has not been renamed into place yet.
    // Reclaiming one would delete an object mid-write, so it is not a candidate.
    if key.ends_with(IN_FLIGHT_SUFFIX)
        || key
            .split('/')
            .any(|segment| segment.ends_with(IN_FLIGHT_SUFFIX))
    {
        return ObjectOwner::Unrecognised;
    }

    let segments: Vec<&str> = key.split('/').collect();
    match segments.as_slice() {
        [collection, record, _filename] => ObjectOwner::Source {
            collection: (*collection).to_string(),
            record: (*record).to_string(),
        },
        [collection, record, "thumbs", filename, suffix] => {
            let Some((preset, format)) = suffix.rsplit_once('_') else {
                return ObjectOwner::Unrecognised;
            };
            // The format has to be one we could have written, or this is some
            // other tool's file and not ours to remove.
            if OutputFormat::parse(format).is_err() || preset.is_empty() {
                return ObjectOwner::Unrecognised;
            }
            ObjectOwner::Thumbnail {
                collection: (*collection).to_string(),
                record: (*record).to_string(),
                source: format!("{collection}/{record}/{filename}"),
                preset: preset.to_string(),
            }
        }
        _ => ObjectOwner::Unrecognised,
    }
}

/// How to run a sweep.
#[derive(Debug, Clone)]
pub struct SweepOptions {
    /// Report what would happen without queueing anything.
    pub dry_run: bool,
    /// Most objects to examine, so one run cannot take an unbounded amount of time.
    pub limit: usize,
}

impl Default for SweepOptions {
    fn default() -> Self {
        Self {
            dry_run: true,
            limit: 10_000,
        }
    }
}

/// What a sweep did, or would do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SweepReport {
    /// Objects examined.
    pub scanned: u64,
    /// Objects found to be unreferenced.
    pub orphans: u64,
    /// Objects queued for deletion. Always zero on a dry run.
    pub enqueued: u64,
    /// Objects whose key this sweeper cannot attribute, which are left alone.
    pub unrecognised: u64,
    /// Whether this was a dry run.
    pub dry_run: bool,
    /// Up to [`SAMPLE`] of the orphan keys, so an operator can look before leaping.
    pub sample: Vec<String>,
}

impl SweepReport {
    /// Whether the limit stopped the run before it saw everything.
    pub fn was_truncated(&self, options: &SweepOptions) -> bool {
        self.scanned as usize >= options.limit
    }
}

/// Find unreferenced objects and, unless this is a dry run, queue them.
pub async fn sweep(
    runner: &dyn QueryRunner,
    storage: &dyn Storage,
    options: &SweepOptions,
) -> Result<SweepReport> {
    let mut report = SweepReport {
        dry_run: options.dry_run,
        ..SweepReport::default()
    };

    let keys = storage.list("").await?;
    let present: HashSet<&str> = keys.iter().map(String::as_str).collect();

    // The collections that exist, so a file under a dropped collection is
    // recognisably orphaned rather than merely unrecognised.
    let collection_rows = runner
        .run_query("select name from _collections", &[])
        .await
        .map_err(db_error)?;
    let collections: HashSet<String> = collection_rows
        .iter()
        .map(|row| row.get::<_, String>("name"))
        .collect();

    let preset_rows = runner
        .run_query("select name from _image_presets", &[])
        .await
        .map_err(db_error)?;
    let presets: HashSet<String> = preset_rows
        .iter()
        .map(|row| row.get::<_, String>("name"))
        .collect();

    // A record's existence is checked once even when it owns several objects.
    let mut record_exists: HashMap<(String, String), bool> = HashMap::new();

    for key in keys.iter() {
        if report.scanned as usize >= options.limit {
            break;
        }
        report.scanned += 1;

        let orphaned = match classify(key) {
            ObjectOwner::Unrecognised => {
                report.unrecognised += 1;
                continue;
            }
            ObjectOwner::Source { collection, record } => {
                if !collections.contains(&collection) {
                    true
                } else {
                    !record_is_present(runner, &mut record_exists, &collection, &record).await?
                }
            }
            ObjectOwner::Thumbnail {
                collection,
                record,
                source,
                preset,
            } => {
                // A thumbnail has nothing left to derive from once its preset is
                // gone, its source is gone, or its record is gone. Stated as one
                // condition because any of the three is sufficient.
                if !presets.contains(&preset)
                    || !present.contains(source.as_str())
                    || !collections.contains(&collection)
                {
                    true
                } else {
                    !record_is_present(runner, &mut record_exists, &collection, &record).await?
                }
            }
        };

        if !orphaned {
            continue;
        }

        report.orphans += 1;
        if report.sample.len() < SAMPLE {
            report.sample.push(key.clone());
        }

        if !options.dry_run {
            outbox::enqueue(runner, key).await?;
            report.enqueued += 1;
        }
    }

    tracing::info!(
        scanned = report.scanned,
        orphans = report.orphans,
        enqueued = report.enqueued,
        dry_run = report.dry_run,
        "storage sweep finished"
    );

    Ok(report)
}

/// Whether a record row still exists, caching the answer per record.
async fn record_is_present(
    runner: &dyn QueryRunner,
    cache: &mut HashMap<(String, String), bool>,
    collection: &str,
    record: &str,
) -> Result<bool> {
    let cache_key = (collection.to_string(), record.to_string());
    if let Some(existing) = cache.get(&cache_key) {
        return Ok(*existing);
    }

    // The collection name comes from the uploaded key, so it is validated before
    // it is interpolated as a table name — and quoted, so a name that happens to
    // be a keyword is still just a name.
    validate_identifier("collection name", collection)?;
    let sql = format!("select 1 from \"{collection}\" where id = $1::text");
    let present = runner
        .run_query_opt(&sql, &[&record.to_string()])
        .await
        .map_err(db_error)?
        .is_some();

    cache.insert(cache_key, present);
    Ok(present)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_file_is_attributed_to_its_record() {
        assert_eq!(
            classify("posts/p1/abc123_photo.png"),
            ObjectOwner::Source {
                collection: "posts".into(),
                record: "p1".into(),
            }
        );
    }

    #[test]
    fn a_thumbnail_is_attributed_to_its_source_and_preset() {
        assert_eq!(
            classify("posts/p1/thumbs/abc123_photo.png/md_webp"),
            ObjectOwner::Thumbnail {
                collection: "posts".into(),
                record: "p1".into(),
                source: "posts/p1/abc123_photo.png".into(),
                preset: "md".into(),
            }
        );
    }

    #[test]
    fn a_preset_name_containing_an_underscore_is_parsed_from_the_last_separator() {
        // Preset names may contain `-` and `_`, so the separator has to be the
        // last one: splitting on the first would read the preset as `my` and the
        // format as `wide_thumb_webp`.
        match classify("posts/p1/thumbs/abc_photo.png/my_wide_thumb_webp") {
            ObjectOwner::Thumbnail { preset, .. } => assert_eq!(preset, "my_wide_thumb"),
            other => panic!("expected a thumbnail, got {other:?}"),
        }
    }

    #[test]
    fn an_in_flight_write_is_never_a_candidate() {
        // Reclaiming a temporary file would delete an object mid-write.
        for key in [
            "posts/p1/abc_photo.part",
            "posts/p1/thumbs/abc_photo.png/md.part",
        ] {
            assert_eq!(classify(key), ObjectOwner::Unrecognised, "{key}");
        }
    }

    #[test]
    fn keys_we_cannot_attribute_are_left_alone() {
        for key in [
            "",
            "loose.png",
            "posts/only-two-segments",
            "posts/p1/thumbs/abc.png",        // no preset suffix
            "posts/p1/thumbs/abc.png/md_bmp", // a format we never wrote
            "posts/p1/thumbs/abc.png/_webp",  // an empty preset name
            "a/b/c/d/e/f",                    // too deep
            "posts/p1/deeper/still/file.png", // not our layout
        ] {
            assert_eq!(classify(key), ObjectOwner::Unrecognised, "{key:?}");
        }
    }

    #[test]
    fn the_default_sweep_is_a_dry_run() {
        // The dangerous direction is deleting, so the default must not.
        assert!(SweepOptions::default().dry_run);
    }

    #[test]
    fn a_truncated_run_says_so() {
        let options = SweepOptions {
            dry_run: true,
            limit: 10,
        };
        let report = SweepReport {
            scanned: 10,
            ..SweepReport::default()
        };
        assert!(report.was_truncated(&options));
        let short = SweepReport {
            scanned: 9,
            ..SweepReport::default()
        };
        assert!(!short.was_truncated(&options));
    }

    #[test]
    fn every_output_format_a_thumbnail_key_can_carry_is_recognised() {
        for format in ["png", "jpeg", "webp"] {
            assert!(
                matches!(
                    classify(&format!("posts/p1/thumbs/abc_photo.png/md_{format}")),
                    ObjectOwner::Thumbnail { .. }
                ),
                "{format} should be recognised"
            );
        }
        // avif is allowed by the schema even though we cannot write it, so a key
        // carrying it is still ours to reason about.
        assert!(matches!(
            classify("posts/p1/thumbs/abc_photo.png/md_avif"),
            ObjectOwner::Thumbnail { .. }
        ));
    }
}
