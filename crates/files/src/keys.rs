//! Object key layout.
//!
//! Keys are `{collection}/{record_id}/{random}_{filename}`: grouping by
//! collection and record makes an orphan sweepable, and the random prefix keeps
//! two uploads of the same name from colliding.

use lazypock_core::error::Result;

use crate::validation::ImageFormat;

/// Longest filename we keep in a key.
const MAX_FILENAME: usize = 80;

/// A filename that is safe inside a key: no separators, no traversal, no
/// surprising characters.
///
/// Anything outside `[A-Za-z0-9._-]` becomes `_`, leading dots are dropped so a
/// name cannot be hidden or traverse, and the result is length-capped. The
/// extension is preserved when it fits, because it is what a browser and a CDN
/// use to pick a content type.
pub fn safe_filename(original: &str) -> String {
    let original = original.rsplit(['/', '\\']).next().unwrap_or(original);

    let (stem, extension) = match original.rsplit_once('.') {
        Some((stem, extension))
            // A stem that is only dots is not a name, so `..hidden` is treated as
            // a whole filename rather than as `.` plus an extension.
            if !stem.is_empty()
                && !stem.chars().all(|character| character == '.')
                && !extension.is_empty()
                && extension.len() <= 10 =>
        {
            (stem, Some(extension))
        }
        _ => (original, None),
    };

    let clean = |value: &str| -> String {
        value
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                    character
                } else {
                    '_'
                }
            })
            .collect()
    };

    let mut name = clean(stem.trim_start_matches('.'));
    if let Some(extension) = extension {
        if !name.is_empty() {
            name.push('.');
            name.push_str(&clean(extension));
        }
    }

    if name.is_empty() {
        name.push_str("file");
    }
    if name.len() > MAX_FILENAME {
        name.truncate(MAX_FILENAME);
    }
    name
}

/// Build the object key for a new upload.
pub fn storage_key(collection: &str, record_id: &str, filename: &str) -> Result<String> {
    // Reuses the identifier rules, so a collection or record id cannot introduce
    // a path segment of its own.
    lazypock_db::types::validate_identifier("collection name", collection)?;
    let record_id = record_id.trim();
    if record_id.is_empty()
        || record_id.contains(['/', '\\'])
        || record_id == "."
        || record_id == ".."
    {
        return Err(lazypock_core::error::Error::invalid(
            "a record id cannot contain a path separator",
        ));
    }

    Ok(format!(
        "{collection}/{record_id}/{}_{}",
        lazypock_core::id::new_id(),
        safe_filename(filename)
    ))
}

/// The content type to serve a format with.
pub fn content_type_for(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::Gif => "image/gif",
        ImageFormat::Webp => "image/webp",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames_are_reduced_to_something_safe() {
        assert_eq!(safe_filename("holiday.png"), "holiday.png");
        assert_eq!(safe_filename("My Photo (1).JPG"), "My_Photo__1_.JPG");
        assert_eq!(safe_filename("../../etc/passwd"), "passwd");
        assert_eq!(safe_filename("..hidden"), "hidden");
        assert_eq!(safe_filename("a/b/c.png"), "c.png");
        assert_eq!(safe_filename("c:\\windows\\evil.png"), "evil.png");
        assert_eq!(safe_filename(""), "file");
        assert_eq!(safe_filename("..."), "file");
    }

    #[test]
    fn a_long_filename_is_capped() {
        let long = format!("{}.png", "a".repeat(300));
        let safe = safe_filename(&long);
        assert!(safe.len() <= MAX_FILENAME, "{} chars", safe.len());
    }

    #[test]
    fn keys_group_by_collection_and_record() {
        let key = storage_key("posts", "abcDEF123456789", "photo.png").expect("key");
        let parts: Vec<&str> = key.split('/').collect();
        assert_eq!(parts.len(), 3, "{key}");
        assert_eq!(parts[0], "posts");
        assert_eq!(parts[1], "abcDEF123456789");
        assert!(parts[2].ends_with("_photo.png"), "{key}");
        // The random prefix means two uploads of the same name never collide.
        let other = storage_key("posts", "abcDEF123456789", "photo.png").expect("key");
        assert_ne!(key, other);
    }

    #[test]
    fn a_traversing_record_id_is_refused() {
        for bad in ["..", ".", "a/b", "a\\b", ""] {
            assert!(
                storage_key("posts", bad, "photo.png").is_err(),
                "{bad:?} should be refused"
            );
        }
        assert!(
            storage_key("Posts", "abc", "photo.png").is_err(),
            "identifier rules apply"
        );
    }
}
