//! Named image presets (`PLAN.md` section 10).
//!
//! A preset is a box, a fit mode and an output format. Rows live in
//! `_image_presets`, whose schema is created by `0001_system_tables.sql`: `name`
//! is the primary key, `fit` carries the imgproxy-style mode, and both
//! dimensions are nullable so a preset may constrain one side only.

use serde::{Deserialize, Serialize};

use lazypock_core::error::{Error, Result};
use lazypock_db::pool::db_error;
use lazypock_db::runner::QueryRunner;

use crate::thumbnail::{OutputFormat, ResizeMode};

/// The largest box a preset may ask for.
pub const MAX_DIMENSION: u32 = 8192;

/// The columns every preset query selects, in the order [`ImagePreset::from_row`]
/// expects.
const COLUMNS: &str = "name, width, height, fit, format, quality";

/// A named size an image can be served at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImagePreset {
    /// The name used in a URL, and the table's primary key.
    pub name: String,
    /// Box width, or `None` to derive it from the source.
    pub width: Option<u32>,
    /// Box height, or `None` to derive it from the source.
    pub height: Option<u32>,
    /// How the source is fitted to the box.
    pub mode: ResizeMode,
    /// The encoding of the result.
    pub format: OutputFormat,
    /// Encoder quality, 1-100.
    pub quality: u8,
}

impl ImagePreset {
    /// Read a preset from a row selected with [`COLUMNS`].
    pub fn from_row(row: &tokio_postgres::Row) -> Result<Self> {
        let name: String = row.get("name");
        let width: Option<i32> = row.get("width");
        let height: Option<i32> = row.get("height");
        let fit: String = row.get("fit");
        let format: String = row.get("format");
        let quality: i32 = row.get("quality");

        let dimension = |value: Option<i32>, axis: &str| -> Result<Option<u32>> {
            match value {
                None => Ok(None),
                Some(value) if value > 0 && value as u32 <= MAX_DIMENSION => Ok(Some(value as u32)),
                Some(value) => Err(Error::config(format!(
                    "image preset {name:?} asks for a {axis} of {value}, \
                     outside 1..={MAX_DIMENSION}"
                ))),
            }
        };

        let (width, height) = (dimension(width, "width")?, dimension(height, "height")?);
        if width.is_none() && height.is_none() {
            // Fail at load rather than at the first request: a preset with no
            // dimensions can never produce anything, and the schema permits it.
            return Err(Error::config(format!(
                "image preset {name:?} gives neither a width nor a height"
            )));
        }
        if !(1..=100).contains(&quality) {
            return Err(Error::config(format!(
                "image preset {name:?} has quality {quality}, outside 1-100"
            )));
        }

        Ok(ImagePreset {
            name,
            width,
            height,
            mode: ResizeMode::parse(&fit)?,
            format: OutputFormat::parse(&format)?,
            quality: quality as u8,
        })
    }

    /// Where the generated thumbnail is cached.
    ///
    /// Derived from the source key and the preset name, so an object at this key
    /// is immutable and can be cached forever.
    ///
    /// The thumbnail is a **sibling** of the source, under a `thumbs/` directory
    /// named after the source file. It cannot be a child of the source key: that
    /// key ends in a filename, which is a file on disk, so nothing can live
    /// beneath it. Namespacing by the source filename keeps the key unique per
    /// source, and keeps every thumbnail of one source attributable to it.
    pub fn thumbnail_key(&self, source_key: &str) -> String {
        let (parent, filename) = source_key.rsplit_once('/').unwrap_or(("", source_key));
        let suffix = format!("{}_{}", self.name, self.format.as_str());
        if parent.is_empty() {
            format!("thumbs/{filename}/{suffix}")
        } else {
            format!("{parent}/thumbs/{filename}/{suffix}")
        }
    }

    /// Parse a `WxH` specification, as accepted by `?thumb=`.
    ///
    /// Fits inside the box without enlarging, which is what a caller who did not
    /// name a stored preset wants: a small source stays sharp rather than being
    /// blown up to the requested size.
    pub fn parse_size(specification: &str) -> Result<Self> {
        if specification.is_empty() {
            return Err(Error::invalid(
                "expected a size like `400x300`, got an empty value",
            ));
        }

        let (width, height) = specification.split_once(['x', 'X']).ok_or_else(|| {
            Error::invalid(format!(
                "expected a size like `400x300`, got {specification:?}"
            ))
        })?;

        let parse = |value: &str, axis: &str| -> Result<u32> {
            let number: u32 = value.trim().parse().map_err(|_| {
                Error::invalid(format!("the thumbnail {axis} must be a whole number"))
            })?;
            if number == 0 || number > MAX_DIMENSION {
                return Err(Error::invalid(format!(
                    "the thumbnail {axis} must be between 1 and {MAX_DIMENSION}"
                )));
            }
            Ok(number)
        };

        Ok(ImagePreset {
            name: specification.to_lowercase(),
            width: Some(parse(width, "width")?),
            height: Some(parse(height, "height")?),
            mode: ResizeMode::Inside,
            format: OutputFormat::Webp,
            quality: 80,
        })
    }
}

/// The presets seeded by migration `0005_image_presets.sql`.
///
/// A small square for a grid tile, then progressively larger boxes for a card and
/// a detail view. The grid tile has its own preset rather than scaling a smaller
/// one, so it stays sharp, and the larger ones never enlarge a small source.
pub fn defaults() -> Vec<ImagePreset> {
    let preset = |name: &str, width: u32, height: u32, mode: ResizeMode| ImagePreset {
        name: name.to_string(),
        width: Some(width),
        height: Some(height),
        mode,
        format: OutputFormat::Webp,
        quality: 80,
    };

    vec![
        preset("thumb", 200, 200, ResizeMode::Cover),
        preset("sm", 320, 320, ResizeMode::Inside),
        preset("md", 768, 768, ResizeMode::Inside),
        preset("lg", 1600, 1600, ResizeMode::Inside),
    ]
}

/// Every preset, ordered by name.
pub async fn load(runner: &dyn QueryRunner) -> Result<Vec<ImagePreset>> {
    let rows = runner
        .run_query(
            &format!("select {COLUMNS} from _image_presets order by name"),
            &[],
        )
        .await
        .map_err(db_error)?;

    rows.iter().map(ImagePreset::from_row).collect()
}

/// Look one preset up by name.
pub async fn find(runner: &dyn QueryRunner, name: &str) -> Result<Option<ImagePreset>> {
    let row = runner
        .run_query_opt(
            &format!("select {COLUMNS} from _image_presets where name = $1::text"),
            &[&name.to_string()],
        )
        .await
        .map_err(db_error)?;

    row.as_ref().map(ImagePreset::from_row).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_specification_is_parsed() {
        let preset = ImagePreset::parse_size("400x300").expect("parse");
        assert_eq!(preset.width, Some(400));
        assert_eq!(preset.height, Some(300));
        // The default never enlarges, so a small source is left alone.
        assert_eq!(preset.mode, ResizeMode::Inside);
        assert_eq!(preset.format, OutputFormat::Webp);
        assert_eq!(
            ImagePreset::parse_size("400X300").expect("parse").width,
            Some(400)
        );
    }

    #[test]
    fn a_malformed_size_is_refused() {
        for bad in [
            "", "400", "400x", "x300", "0x300", "400x0", "a x b", "99999x10",
        ] {
            assert!(
                ImagePreset::parse_size(bad).is_err(),
                "{bad:?} should be refused"
            );
        }
    }

    #[test]
    fn a_thumbnail_key_is_derived_from_the_source_and_the_preset() {
        let preset = ImagePreset::parse_size("400x300").expect("parse");
        let key = preset.thumbnail_key("posts/p1/abc_photo.png");
        assert_eq!(key, "posts/p1/thumbs/abc_photo.png/400x300_webp");

        // Two presets over the same source never collide.
        let other = ImagePreset::parse_size("100x100").expect("parse");
        assert_ne!(other.thumbnail_key("posts/p1/abc_photo.png"), key);
    }

    #[test]
    fn a_thumbnail_is_a_sibling_of_its_source_never_a_child() {
        // A child would be impossible: the source key ends in a filename, which
        // is a file on disk, and nothing can be stored beneath it.
        let preset = defaults().remove(0);
        let source = "posts/p1/abc_photo.png";
        let key = preset.thumbnail_key(source);

        assert_ne!(key, source, "a thumbnail must not overwrite its source");
        assert!(
            !key.starts_with(&format!("{source}/")),
            "the thumbnail must not be nested under the source file: {key}"
        );
        // Same parent, so sweeping one record's storage prefix reaches both.
        let parent = |key: &str| key.rsplit_once('/').expect("a parent").0.to_string();
        assert_eq!(
            parent(&key),
            "posts/p1/thumbs/abc_photo.png".to_string(),
            "the thumbnail is namespaced by its source"
        );
        // A source with no directory still produces a usable key.
        assert_eq!(
            preset.thumbnail_key("photo.png"),
            "thumbs/photo.png/thumb_webp"
        );
    }

    #[test]
    fn the_seeded_presets_are_sane_and_distinct() {
        let defaults = defaults();
        assert!(!defaults.is_empty());

        let mut names: Vec<&str> = defaults.iter().map(|preset| preset.name.as_str()).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "preset names must be unique");

        for preset in &defaults {
            let width = preset.width.expect("a seeded width");
            let height = preset.height.expect("a seeded height");
            assert!(width > 0 && width <= MAX_DIMENSION, "{preset:?}");
            assert!(height > 0 && height <= MAX_DIMENSION, "{preset:?}");
            assert!((1..=100).contains(&preset.quality), "{preset:?}");
            assert_ne!(
                preset.format,
                OutputFormat::Avif,
                "the seeded presets must not ask for a format we cannot yet write"
            );
        }

        // A grid tile preset exists and is square, so tiles line up.
        let thumb = defaults
            .iter()
            .find(|preset| preset.name == "thumb")
            .expect("a thumb preset");
        assert_eq!(thumb.width, thumb.height);
    }

    #[test]
    fn the_seeded_sizes_increase_and_never_enlarge() {
        let defaults = defaults();
        let width = |name: &str| {
            defaults
                .iter()
                .find(|preset| preset.name == name)
                .expect("preset")
                .width
                .expect("width")
        };
        assert!(width("thumb") < width("sm"), "tiles should not be largest");
        assert!(width("sm") < width("md"));
        assert!(width("md") < width("lg"));

        // Everything except the square tile is `inside`, so a small source is
        // never blown up to a preset's size.
        for preset in &defaults {
            if preset.name != "thumb" {
                assert_eq!(
                    preset.mode,
                    ResizeMode::Inside,
                    "{} should not enlarge a small source",
                    preset.name
                );
            }
        }
    }
}
