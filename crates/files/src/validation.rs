//! What an upload must look like before anything is stored.
//!
//! The format comes from the **bytes**, never from the filename or the declared
//! content type: both are attacker-controlled. SVG is refused outright — it is
//! served as an image but executes script, so accepting it would be an XSS hole
//! in every origin that serves objects.

use bytes::Bytes;
use lazypock_core::error::{Error, Result};

/// The formats we accept, and can measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    /// PNG.
    Png,
    /// JPEG.
    Jpeg,
    /// GIF.
    Gif,
    /// WebP.
    Webp,
}

impl ImageFormat {
    /// Short name, for errors and field options.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Gif => "gif",
            Self::Webp => "webp",
        }
    }

    /// Content type to serve it with.
    pub fn content_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
        }
    }

    /// Parse a format name, as configured on a field.
    pub fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "png" => Ok(Self::Png),
            "jpg" | "jpeg" => Ok(Self::Jpeg),
            "gif" => Ok(Self::Gif),
            "webp" => Ok(Self::Webp),
            other => Err(Error::config(format!(
                "unsupported image type {other:?}; supported: png, jpeg, gif, webp"
            ))),
        }
    }
}

impl std::fmt::Display for ImageFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Limits for one upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadLimits {
    /// Largest accepted body.
    pub max_bytes: u64,
    /// Largest permitted width or height.
    ///
    /// A guard against decompression bombs: a few hundred bytes can declare a
    /// hundred-thousand-pixel image, and decoding it would take the process down.
    pub max_dimension: u32,
    /// Permitted formats; empty means every format we can measure.
    pub allowed: Vec<ImageFormat>,
}

impl Default for UploadLimits {
    fn default() -> Self {
        Self {
            max_bytes: 5 * 1024 * 1024,
            max_dimension: 8192,
            allowed: Vec::new(),
        }
    }
}

/// Sniff the format from the bytes themselves.
pub fn sniff(content: &[u8]) -> Option<ImageFormat> {
    if content.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(ImageFormat::Png);
    }
    if content.starts_with(b"\xFF\xD8\xFF") {
        return Some(ImageFormat::Jpeg);
    }
    if content.starts_with(b"GIF87a") || content.starts_with(b"GIF89a") {
        return Some(ImageFormat::Gif);
    }
    if content.len() >= 12 && &content[0..4] == b"RIFF" && &content[8..12] == b"WEBP" {
        return Some(ImageFormat::Webp);
    }
    None
}

/// Whether the bytes look like SVG or XML, which is refused however they are
/// labelled.
fn looks_like_svg(content: &[u8]) -> bool {
    // Only the head is inspected: a file that opens with XML is not an image, and
    // an image format never starts with `<`.
    let head = &content[..content.len().min(512)];
    let text = String::from_utf8_lossy(head).to_ascii_lowercase();
    let trimmed = text.trim_start();
    trimmed.starts_with("<?xml") || trimmed.starts_with("<svg") || trimmed.contains("<svg")
}

/// Read the pixel dimensions without decoding the image.
pub fn dimensions(content: &[u8], format: ImageFormat) -> Option<(u32, u32)> {
    match format {
        ImageFormat::Png => png_dimensions(content),
        ImageFormat::Gif => gif_dimensions(content),
        ImageFormat::Jpeg => jpeg_dimensions(content),
        ImageFormat::Webp => webp_dimensions(content),
    }
}

fn png_dimensions(content: &[u8]) -> Option<(u32, u32)> {
    // 8-byte signature, 4-byte chunk length, 4-byte "IHDR", then width/height.
    if content.len() < 24 || &content[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(content[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(content[20..24].try_into().ok()?);
    Some((width, height))
}

fn gif_dimensions(content: &[u8]) -> Option<(u32, u32)> {
    if content.len() < 10 {
        return None;
    }
    let width = u16::from_le_bytes(content[6..8].try_into().ok()?) as u32;
    let height = u16::from_le_bytes(content[8..10].try_into().ok()?) as u32;
    Some((width, height))
}

fn webp_dimensions(content: &[u8]) -> Option<(u32, u32)> {
    if content.len() < 30 {
        return None;
    }
    match &content[12..16] {
        // Lossy: a 14-bit width and height follow a 3-byte frame tag.
        b"VP8 " => {
            let width = u16::from_le_bytes(content[26..28].try_into().ok()?) as u32 & 0x3FFF;
            let height = u16::from_le_bytes(content[28..30].try_into().ok()?) as u32 & 0x3FFF;
            Some((width, height))
        }
        // Lossless: packed after the 0x2f signature byte.
        b"VP8L" => {
            let bits = u32::from_le_bytes(content[21..25].try_into().ok()?);
            let width = (bits & 0x3FFF) + 1;
            let height = ((bits >> 14) & 0x3FFF) + 1;
            Some((width, height))
        }
        // Extended: 24-bit little-endian canvas size, stored as size - 1.
        b"VP8X" => {
            let width = u32::from_le_bytes([content[24], content[25], content[26], 0]) + 1;
            let height = u32::from_le_bytes([content[27], content[28], content[29], 0]) + 1;
            Some((width, height))
        }
        _ => None,
    }
}

fn jpeg_dimensions(content: &[u8]) -> Option<(u32, u32)> {
    let mut index = 2;
    while index + 9 < content.len() {
        if content[index] != 0xFF {
            index += 1;
            continue;
        }
        let marker = content[index + 1];
        // Padding and standalone markers carry no length.
        if marker == 0xFF || marker == 0x01 || (0xD0..=0xD9).contains(&marker) {
            index += 2;
            continue;
        }
        // SOF0..SOF15, except the ones that are not frame headers.
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let height = u16::from_be_bytes(content[index + 5..index + 7].try_into().ok()?) as u32;
            let width = u16::from_be_bytes(content[index + 7..index + 9].try_into().ok()?) as u32;
            return Some((width, height));
        }
        let length = u16::from_be_bytes(content[index + 2..index + 4].try_into().ok()?) as usize;
        if length < 2 {
            return None;
        }
        index += 2 + length;
    }
    None
}

/// Validate an upload, returning the format it actually is.
///
/// `declared_type` is only used to explain a mismatch in the error message; it is
/// never trusted.
pub fn validate(
    content: &Bytes,
    declared_type: &str,
    limits: &UploadLimits,
) -> Result<ImageFormat> {
    if content.is_empty() {
        return Err(Error::invalid("the uploaded file is empty"));
    }
    if content.len() as u64 > limits.max_bytes {
        return Err(Error::invalid(format!(
            "the uploaded file is {} bytes, more than the {} byte limit",
            content.len(),
            limits.max_bytes
        )));
    }
    if looks_like_svg(content) {
        return Err(Error::invalid(
            "SVG is not accepted: it is served as an image but can execute script",
        ));
    }

    let format = sniff(content).ok_or_else(|| {
        Error::invalid(format!(
            "the file is not a supported image (declared as {declared_type:?}); \
             supported formats are png, jpeg, gif and webp"
        ))
    })?;

    if !limits.allowed.is_empty() && !limits.allowed.contains(&format) {
        let allowed: Vec<&str> = limits
            .allowed
            .iter()
            .map(|format| format.as_str())
            .collect();
        return Err(Error::invalid(format!(
            "a {format} file is not accepted here; allowed: {}",
            allowed.join(", ")
        )));
    }

    let (width, height) = dimensions(content, format).ok_or_else(|| {
        Error::invalid(format!(
            "the {format} file's dimensions could not be read, so it will not be stored"
        ))
    })?;
    if width == 0 || height == 0 {
        return Err(Error::invalid(format!(
            "the {format} file has a zero dimension ({width}x{height})"
        )));
    }
    if width > limits.max_dimension || height > limits.max_dimension {
        return Err(Error::invalid(format!(
            "the image is {width}x{height}, larger than the {} pixel limit",
            limits.max_dimension
        )));
    }

    Ok(format)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG header with the given dimensions.
    fn png(width: u32, height: u32) -> Bytes {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(&13u32.to_be_bytes()); // IHDR length
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]); // bit depth, colour type, ...
        Bytes::from(bytes)
    }

    fn gif(width: u16, height: u16) -> Bytes {
        let mut bytes = b"GIF89a".to_vec();
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        Bytes::from(bytes)
    }

    fn webp_vp8x(width: u32, height: u32) -> Bytes {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(b"WEBP");
        bytes.extend_from_slice(b"VP8X");
        bytes.extend_from_slice(&10u32.to_le_bytes());
        bytes.extend_from_slice(&[0, 0, 0, 0]); // flags + reserved
        bytes.extend_from_slice(&(width - 1).to_le_bytes()[..3]);
        bytes.extend_from_slice(&(height - 1).to_le_bytes()[..3]);
        Bytes::from(bytes)
    }

    #[test]
    fn formats_are_sniffed_from_the_bytes() {
        assert_eq!(sniff(&png(1, 1)), Some(ImageFormat::Png));
        assert_eq!(sniff(&gif(1, 1)), Some(ImageFormat::Gif));
        assert_eq!(sniff(&webp_vp8x(1, 1)), Some(ImageFormat::Webp));
        assert_eq!(sniff(b"\xFF\xD8\xFF\xE0"), Some(ImageFormat::Jpeg));
        assert_eq!(sniff(b"not an image"), None);
        assert_eq!(sniff(b""), None);
    }

    #[test]
    fn dimensions_are_read_without_decoding() {
        assert_eq!(
            dimensions(&png(800, 600), ImageFormat::Png),
            Some((800, 600))
        );
        assert_eq!(
            dimensions(&gif(320, 240), ImageFormat::Gif),
            Some((320, 240))
        );
        assert_eq!(
            dimensions(&webp_vp8x(1000, 500), ImageFormat::Webp),
            Some((1000, 500))
        );
    }

    #[test]
    fn a_valid_upload_is_accepted() {
        let limits = UploadLimits::default();
        assert_eq!(
            validate(&png(100, 100), "image/png", &limits).unwrap(),
            ImageFormat::Png
        );
    }

    #[test]
    fn svg_is_refused_even_when_it_claims_to_be_a_png() {
        let limits = UploadLimits::default();
        for svg in [
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>".to_vec(),
            b"<?xml version=\"1.0\"?><svg/>".to_vec(),
            b"\n  <SVG onload=\"alert(1)\">".to_vec(),
        ] {
            let err = validate(&Bytes::from(svg.clone()), "image/png", &limits).unwrap_err();
            assert!(err.message().contains("SVG"), "{err}");
        }
    }

    #[test]
    fn a_declared_type_that_disagrees_with_the_bytes_is_irrelevant() {
        // A GIF claiming to be a PNG is a GIF; the bytes win.
        let limits = UploadLimits::default();
        assert_eq!(
            validate(&gif(10, 10), "image/png", &limits).unwrap(),
            ImageFormat::Gif
        );
    }

    #[test]
    fn non_images_and_empty_bodies_are_refused() {
        let limits = UploadLimits::default();
        assert!(validate(&Bytes::from_static(b""), "image/png", &limits).is_err());
        assert!(validate(
            &Bytes::from_static(b"#!/bin/sh\nrm -rf /"),
            "image/png",
            &limits
        )
        .is_err());
    }

    #[test]
    fn the_size_limit_is_enforced() {
        let limits = UploadLimits {
            max_bytes: 10,
            ..Default::default()
        };
        let err = validate(&png(1, 1), "image/png", &limits).unwrap_err();
        assert!(err.message().contains("more than"), "{err}");
    }

    #[test]
    fn a_decompression_bomb_is_refused_by_the_dimension_limit() {
        let limits = UploadLimits {
            max_dimension: 1000,
            ..Default::default()
        };
        // A few dozen bytes claiming to be 50,000 pixels wide.
        let err = validate(&png(50_000, 50_000), "image/png", &limits).unwrap_err();
        assert!(err.message().contains("larger than"), "{err}");
    }

    #[test]
    fn a_zero_dimension_is_refused() {
        let limits = UploadLimits::default();
        assert!(validate(&png(0, 100), "image/png", &limits).is_err());
    }

    #[test]
    fn a_field_allowlist_is_enforced() {
        let limits = UploadLimits {
            allowed: vec![ImageFormat::Png],
            ..Default::default()
        };
        assert!(validate(&png(10, 10), "image/png", &limits).is_ok());
        let err = validate(&gif(10, 10), "image/gif", &limits).unwrap_err();
        assert!(err.message().contains("not accepted"), "{err}");
    }

    #[test]
    fn a_format_we_cannot_measure_is_refused_rather_than_stored() {
        // A RIFF container that is not a WebP we understand.
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(b"WEBP");
        bytes.extend_from_slice(b"XXXX");
        bytes.extend_from_slice(&[0u8; 20]);
        let err =
            validate(&Bytes::from(bytes), "image/webp", &UploadLimits::default()).unwrap_err();
        assert!(
            err.message().contains("dimensions could not be read"),
            "{err}"
        );
    }

    #[test]
    fn format_names_round_trip() {
        for format in [
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::Gif,
            ImageFormat::Webp,
        ] {
            assert_eq!(ImageFormat::parse(format.as_str()).unwrap(), format);
            assert!(format.content_type().starts_with("image/"));
        }
        assert_eq!(ImageFormat::parse("JPG").unwrap(), ImageFormat::Jpeg);
        assert!(ImageFormat::parse("tiff").is_err());
    }
}
