//! Thumbnail generation.
//!
//! Two ways to produce one, and the configuration picks between them:
//!
//! * **In this process** (the default): decoding, resizing and re-encoding with the
//!   `image` codecs, compiled in behind the `local-image-codecs` feature. No sidecar
//!   and no external service, which is what keeps the binary self-contained.
//! * **An external service**, when `LAZYPOCK_IMAGE_SERVICE_URL` is set. The CPU and
//!   the peak memory then belong to that service — see [`crate::image_service`] for
//!   the contract, and build with `--no-default-features` to drop the codecs from
//!   this binary entirely.
//!
//! The `fit` vocabulary is the one `_image_presets.fit` stores, so a value an
//! operator writes by hand means here what it means in the schema.

#[cfg(feature = "local-image-codecs")]
use image::imageops::FilterType;
#[cfg(feature = "local-image-codecs")]
use image::{DynamicImage, ImageEncoder};
use lazypock_core::error::{Error, Result};

use crate::image_service::ImageService;
use crate::validation::ImageFormat;

/// How a source image is fitted to the target box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResizeMode {
    /// Fill the box exactly by cropping the overflow. The aspect ratio is
    /// preserved, so nothing is distorted.
    Cover,
    /// Fit inside the box, preserving the aspect ratio, enlarging a small source
    /// when asked to.
    Contain,
    /// Fill the box exactly, ignoring the aspect ratio. This one distorts.
    Fill,
    /// Fit inside the box, preserving the aspect ratio, **never enlarging**.
    Inside,
    /// Fit so the result covers the box in both dimensions, preserving the aspect
    /// ratio and cropping nothing.
    Outside,
}

impl ResizeMode {
    /// The name stored in `_image_presets.fit`.
    pub fn as_str(self) -> &'static str {
        match self {
            ResizeMode::Cover => "cover",
            ResizeMode::Contain => "contain",
            ResizeMode::Fill => "fill",
            ResizeMode::Inside => "inside",
            ResizeMode::Outside => "outside",
        }
    }

    /// Parse a stored name.
    ///
    /// These are exactly the values `_image_presets.fit` allows, so a row that
    /// passes the check constraint always parses.
    pub fn parse(name: &str) -> Result<Self> {
        match name {
            "cover" => Ok(ResizeMode::Cover),
            "contain" => Ok(ResizeMode::Contain),
            "fill" => Ok(ResizeMode::Fill),
            "inside" => Ok(ResizeMode::Inside),
            "outside" => Ok(ResizeMode::Outside),
            other => Err(Error::config(format!(
                "unknown fit mode {other:?}; expected one of \
                 cover, contain, fill, inside, outside"
            ))),
        }
    }

    /// Whether this mode can enlarge a source smaller than the box.
    fn enlarges(self) -> bool {
        matches!(
            self,
            ResizeMode::Contain | ResizeMode::Outside | ResizeMode::Fill
        )
    }
}

/// The encodings a preset may ask for, matching `_image_presets.format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    /// PNG.
    Png,
    /// JPEG.
    Jpeg,
    /// WebP.
    Webp,
    /// AVIF.
    Avif,
}

impl OutputFormat {
    /// The name stored in `_image_presets.format`.
    pub fn as_str(self) -> &'static str {
        match self {
            OutputFormat::Png => "png",
            OutputFormat::Jpeg => "jpeg",
            OutputFormat::Webp => "webp",
            OutputFormat::Avif => "avif",
        }
    }

    /// Parse a stored name.
    pub fn parse(name: &str) -> Result<Self> {
        match name {
            "png" => Ok(OutputFormat::Png),
            "jpeg" | "jpg" => Ok(OutputFormat::Jpeg),
            "webp" => Ok(OutputFormat::Webp),
            "avif" => Ok(OutputFormat::Avif),
            other => Err(Error::config(format!(
                "unknown thumbnail format {other:?}; expected png, jpeg, webp or avif"
            ))),
        }
    }

    /// The content type to serve a result with.
    pub fn content_type(self) -> &'static str {
        match self {
            OutputFormat::Png => "image/png",
            OutputFormat::Jpeg => "image/jpeg",
            OutputFormat::Webp => "image/webp",
            OutputFormat::Avif => "image/avif",
        }
    }

    /// The format to produce when a caller gave no format.
    pub fn from_source(source: ImageFormat) -> Self {
        match source {
            ImageFormat::Png => OutputFormat::Png,
            ImageFormat::Jpeg => OutputFormat::Jpeg,
            ImageFormat::Webp => OutputFormat::Webp,
            // A single GIF frame is served as PNG rather than flattened to JPEG,
            // so transparency survives.
            ImageFormat::Gif => OutputFormat::Png,
        }
    }
}

/// The filter used for every resize.
///
/// Lanczos is the slowest option and the only one that stays sharp on a large
/// downscale, which is what a thumbnail is. Only the in-process codecs have it:
/// a service configured through `LAZYPOCK_IMAGE_SERVICE_URL` picks its own.
#[cfg(feature = "local-image-codecs")]
const FILTER: FilterType = FilterType::Lanczos3;

/// What to produce.
///
/// A missing dimension is derived from the source, so a preset may constrain one
/// side only.
#[derive(Debug, Clone, Copy)]
pub struct ResizeRequest<'a> {
    /// The encoded source.
    pub source: &'a [u8],
    /// The source's format, already sniffed from the bytes.
    pub source_format: ImageFormat,
    /// Box width, or `None` to derive it from the source.
    pub width: Option<u32>,
    /// Box height, or `None` to derive it from the source.
    pub height: Option<u32>,
    /// How to fit the box.
    pub mode: ResizeMode,
    /// The encoding of the result.
    pub format: OutputFormat,
    /// Encoder quality, 1-100.
    pub quality: u8,
}

/// The quality range every path enforces.
///
/// Shared so a preset cannot be valid in one deployment and refused in another:
/// the local codecs and an external service both go through this.
fn check_quality(quality: u8) -> Result<()> {
    if !(1..=100).contains(&quality) {
        return Err(Error::config(format!(
            "thumbnail quality {quality} is out of range; expected 1-100"
        )));
    }
    Ok(())
}

/// Resize and re-encode.
///
/// The configured service does the work when there is one; otherwise the local
/// codecs do. Both paths are checked against the same rules (the quality range), so
/// a preset cannot be valid in one deployment and refused in another.
pub async fn resize(request: ResizeRequest<'_>, service: Option<&ImageService>) -> Result<Vec<u8>> {
    check_quality(request.quality)?;

    match service {
        Some(service) => service.resize(&request).await,
        None => resize_local(request),
    }
}

/// The in-process path.
#[cfg(feature = "local-image-codecs")]
fn resize_local(request: ResizeRequest<'_>) -> Result<Vec<u8>> {
    check_quality(request.quality)?;

    let decoded =
        image::load_from_memory_with_format(request.source, decode_format(request.source_format))
            .map_err(|err| Error::invalid(format!("cannot decode the image: {err}")))?;

    let box_size = resolve_box(
        decoded.width(),
        decoded.height(),
        request.width,
        request.height,
    )?;
    let output = fit(&decoded, box_size, request.mode);

    tracing::debug!(
        source = format!("{}x{}", decoded.width(), decoded.height()),
        target = format!("{}x{}", output.width(), output.height()),
        fit = request.mode.as_str(),
        format = request.format.as_str(),
        "generated a thumbnail"
    );

    encode(&output, request.format, request.quality)
}

/// The in-process path in a build without codecs: a loud refusal.
///
/// It names both ways out instead of returning something empty, because a thumbnail
/// that silently does not happen is discovered by a user staring at a broken image,
/// and "no image codecs in this build" is the actual cause.
#[cfg(not(feature = "local-image-codecs"))]
fn resize_local(_request: ResizeRequest<'_>) -> Result<Vec<u8>> {
    Err(Error::config(
        "this build was compiled without image codecs (`local-image-codecs`): set \
         LAZYPOCK_IMAGE_SERVICE_URL to an image service, or rebuild with the default features",
    ))
}

/// The box to fit the source into, deriving a missing dimension from the source.
#[cfg(feature = "local-image-codecs")]
fn resolve_box(
    source_width: u32,
    source_height: u32,
    width: Option<u32>,
    height: Option<u32>,
) -> Result<(u32, u32)> {
    let check = |value: u32, axis: &str| -> Result<u32> {
        if value == 0 || value > crate::presets::MAX_DIMENSION {
            return Err(Error::invalid(format!(
                "the thumbnail {axis} must be between 1 and {}",
                crate::presets::MAX_DIMENSION
            )));
        }
        Ok(value)
    };

    match (width, height) {
        (Some(width), Some(height)) => Ok((check(width, "width")?, check(height, "height")?)),
        (Some(width), None) => {
            let width = check(width, "width")?;
            let ratio = source_height as f64 / source_width as f64;
            Ok((width, ((width as f64 * ratio).round() as u32).max(1)))
        }
        (None, Some(height)) => {
            let height = check(height, "height")?;
            let ratio = source_width as f64 / source_height as f64;
            Ok((((height as f64 * ratio).round() as u32).max(1), height))
        }
        (None, None) => Err(Error::config(
            "an image preset must give a width, a height, or both",
        )),
    }
}

/// Apply the fit mode against an already-resolved box.
#[cfg(feature = "local-image-codecs")]
fn fit(
    source: &DynamicImage,
    (box_width, box_height): (u32, u32),
    mode: ResizeMode,
) -> DynamicImage {
    let (source_width, source_height) = (source.width(), source.height());

    // `fill` is the one mode that ignores the aspect ratio.
    if mode == ResizeMode::Fill {
        return source.resize_exact(box_width, box_height, FILTER);
    }

    let horizontal = box_width as f64 / source_width as f64;
    let vertical = box_height as f64 / source_height as f64;
    let mut scale = match mode {
        ResizeMode::Cover | ResizeMode::Outside => horizontal.max(vertical),
        ResizeMode::Contain | ResizeMode::Inside => horizontal.min(vertical),
        ResizeMode::Fill => unreachable!("handled above"),
    };
    if !mode.enlarges() {
        // `inside` and `cover` never blow a small source up into something blurry.
        // For `cover` this only matters when the source is smaller than the box.
        scale = scale.min(1.0);
    }

    let target_width = ((source_width as f64 * scale).round() as u32).max(1);
    let target_height = ((source_height as f64 * scale).round() as u32).max(1);

    let scaled = if (target_width, target_height) == (source_width, source_height) {
        // Already the right size: re-encoding would only lose quality.
        source.clone()
    } else {
        source.resize_exact(target_width, target_height, FILTER)
    };

    if mode == ResizeMode::Cover {
        // Crop the overflow from the centre, which keeps the subject in frame.
        //
        // This has to happen even when no scaling was needed. A source that is
        // already the right height but too wide needs no resize and still needs a
        // crop, and returning early there is how a `cover` ends up serving the
        // full-size original instead of a tile.
        let x = (target_width.saturating_sub(box_width)) / 2;
        let y = (target_height.saturating_sub(box_height)) / 2;
        return scaled.crop_imm(
            x,
            y,
            box_width.min(target_width),
            box_height.min(target_height),
        );
    }

    scaled
}

/// Encode to the requested format.
#[cfg(feature = "local-image-codecs")]
fn encode(image: &DynamicImage, format: OutputFormat, quality: u8) -> Result<Vec<u8>> {
    let mut output = Vec::new();

    match format {
        OutputFormat::Png => image
            .write_to(
                &mut std::io::Cursor::new(&mut output),
                image::ImageFormat::Png,
            )
            .map_err(|err| Error::internal(format!("cannot encode the thumbnail: {err}")))?,
        OutputFormat::Jpeg => {
            // JPEG has no alpha channel, so an image with transparency is
            // flattened to RGB rather than failing to encode.
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, quality);
            encoder
                .write_image(
                    image.to_rgb8().as_raw(),
                    image.width(),
                    image.height(),
                    image::ExtendedColorType::Rgb8,
                )
                .map_err(|err| Error::internal(format!("cannot encode the thumbnail: {err}")))?;
        }
        OutputFormat::Webp => {
            // The encoder available here is lossless: a larger thumbnail than a
            // lossy encoder would give, but correct and bounded by the source.
            let rgba = image.to_rgba8();
            let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut output);
            encoder
                .write_image(
                    rgba.as_raw(),
                    rgba.width(),
                    rgba.height(),
                    image::ExtendedColorType::Rgba8,
                )
                .map_err(|err| Error::internal(format!("cannot encode the thumbnail: {err}")))?;
        }
        // Refused loudly: the schema allows `avif`, so a preset can ask for it,
        // and silently producing something else would be worse than failing.
        OutputFormat::Avif => {
            return Err(Error::unsupported(
                "AVIF output is not implemented; use webp, jpeg or png",
            ))
        }
    }

    Ok(output)
}

#[cfg(feature = "local-image-codecs")]
fn decode_format(format: ImageFormat) -> image::ImageFormat {
    match format {
        ImageFormat::Png => image::ImageFormat::Png,
        ImageFormat::Jpeg => image::ImageFormat::Jpeg,
        ImageFormat::Gif => image::ImageFormat::Gif,
        ImageFormat::Webp => image::ImageFormat::WebP,
    }
}

#[cfg(all(test, feature = "local-image-codecs"))]
mod tests {
    use super::*;
    use crate::validation::{dimensions, sniff, validate, UploadLimits};
    use bytes::Bytes;

    /// A solid-colour PNG of the given size, produced by our own encoder.
    fn solid(width: u32, height: u32) -> Vec<u8> {
        let image = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            width,
            height,
            image::Rgba([200, 30, 90, 255]),
        ));
        encode(&image, OutputFormat::Png, 100).expect("encode")
    }

    /// Three vertical bands: red on the outside, blue in the middle.
    fn banded(width: u32, height: u32) -> Vec<u8> {
        let mut raw = image::RgbaImage::new(width, height);
        let edge = width / 8;
        for (x, _, pixel) in raw.enumerate_pixels_mut() {
            *pixel = if x < edge || x >= width - edge {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            };
        }
        encode(&DynamicImage::ImageRgba8(raw), OutputFormat::Png, 100).expect("encode")
    }

    fn run(source: &[u8], width: Option<u32>, height: Option<u32>, mode: ResizeMode) -> Vec<u8> {
        resize_local(ResizeRequest {
            source,
            source_format: ImageFormat::Png,
            width,
            height,
            mode,
            format: OutputFormat::Png,
            quality: 80,
        })
        .expect("resize")
    }

    fn size(bytes: &[u8]) -> (u32, u32) {
        dimensions(bytes, ImageFormat::Png).expect("dimensions")
    }

    #[test]
    fn every_mode_the_schema_allows_is_supported() {
        // The check constraint on `_image_presets.fit` and this parser must agree,
        // or a row that the database accepts would fail at request time.
        for name in ["cover", "contain", "fill", "inside", "outside"] {
            assert_eq!(ResizeMode::parse(name).expect("parse").as_str(), name);
        }
        assert!(ResizeMode::parse("squash").is_err());
    }

    #[test]
    fn cover_fills_the_box_and_crops_rather_than_distorting() {
        assert_eq!(
            size(&run(
                &solid(400, 200),
                Some(100),
                Some(100),
                ResizeMode::Cover
            )),
            (100, 100)
        );
    }

    #[test]
    fn contain_fits_inside_the_box_preserving_the_ratio() {
        assert_eq!(
            size(&run(
                &solid(400, 200),
                Some(100),
                Some(100),
                ResizeMode::Contain
            )),
            (100, 50)
        );
    }

    #[test]
    fn fill_ignores_the_aspect_ratio() {
        // The documented cost of `fill`: it distorts, and this asserts that it
        // really does produce exactly the box rather than quietly behaving like
        // `contain`.
        assert_eq!(
            size(&run(
                &solid(400, 200),
                Some(100),
                Some(100),
                ResizeMode::Fill
            )),
            (100, 100)
        );
    }

    #[test]
    fn inside_never_enlarges_and_contain_does() {
        let small = solid(64, 32);
        assert_eq!(
            size(&run(&small, Some(400), Some(400), ResizeMode::Inside)),
            (64, 32),
            "inside must leave a small source alone"
        );
        assert_eq!(
            size(&run(&small, Some(400), Some(400), ResizeMode::Contain)),
            (400, 200),
            "contain enlarges when asked"
        );
    }

    #[test]
    fn outside_covers_the_box_without_cropping() {
        let bytes = run(&banded(400, 200), Some(100), Some(100), ResizeMode::Outside);
        let (width, height) = size(&bytes);
        // Scaled by the dimension that needs the larger factor (200/100), so the
        // result covers the box rather than being cropped into it.
        assert_eq!((width, height), (200, 100));
        assert!(
            width >= 100 && height >= 100,
            "an `outside` result must cover the box, got {width}x{height}"
        );
        // Nothing was cropped, so the red bands must still be present.
        let decoded = image::load_from_memory(&bytes).expect("decode").to_rgb8();
        assert!(
            decoded
                .pixels()
                .any(|pixel| pixel[0] > 200 && pixel[2] < 60),
            "the edges should have survived an `outside` resize"
        );
    }

    #[test]
    fn cover_crops_even_when_the_scale_is_exactly_one() {
        // The case a resize-then-crop shortcut gets wrong: the source is already
        // the box's height, so nothing needs scaling, and it is still far too wide
        // and must be cropped.
        let bytes = run(&banded(400, 200), Some(200), Some(200), ResizeMode::Cover);
        assert_eq!(size(&bytes), (200, 200));
        let decoded = image::load_from_memory(&bytes).expect("decode").to_rgb8();
        for pixel in decoded.pixels() {
            assert!(
                pixel[2] > 200 && pixel[0] < 60,
                "the red bands should have been cropped away, got {:?}",
                (pixel[0], pixel[1], pixel[2])
            );
        }
    }

    #[test]
    fn cover_crops_from_the_centre() {
        let bytes = run(&banded(400, 200), Some(100), Some(100), ResizeMode::Cover);
        assert_eq!(size(&bytes), (100, 100));
        let decoded = image::load_from_memory(&bytes).expect("decode").to_rgb8();
        for pixel in decoded.pixels() {
            assert!(
                pixel[2] > 200 && pixel[0] < 60,
                "the red bands should have been cropped away, got {:?}",
                (pixel[0], pixel[1], pixel[2])
            );
        }
    }

    #[test]
    fn a_one_sided_box_derives_the_other_dimension() {
        // `_image_presets.width` and `.height` are both nullable, so this is a
        // shape an operator can actually write.
        assert_eq!(
            size(&run(&solid(400, 200), Some(100), None, ResizeMode::Inside)),
            (100, 50)
        );
        assert_eq!(
            size(&run(&solid(400, 200), None, Some(100), ResizeMode::Inside)),
            (200, 100)
        );
    }

    #[test]
    fn a_preset_with_no_dimensions_is_refused() {
        let err = resize_local(ResizeRequest {
            source: &solid(64, 64),
            source_format: ImageFormat::Png,
            width: None,
            height: None,
            mode: ResizeMode::Inside,
            format: OutputFormat::Png,
            quality: 80,
        })
        .expect_err("no dimensions should be refused");
        assert!(err.message().contains("width"), "{}", err.message());
    }

    #[test]
    fn a_zero_or_absurd_dimension_is_refused() {
        for (width, height) in [
            (Some(0), Some(10)),
            (Some(10), Some(0)),
            (Some(99999), Some(10)),
        ] {
            let err = resize_local(ResizeRequest {
                source: &solid(64, 64),
                source_format: ImageFormat::Png,
                width,
                height,
                mode: ResizeMode::Inside,
                format: OutputFormat::Png,
                quality: 80,
            })
            .expect_err("out of range should be refused");
            assert!(err.message().contains("between 1 and"), "{}", err.message());
        }
    }

    #[test]
    fn the_result_is_still_a_valid_image_under_our_own_upload_rules() {
        let bytes = run(&solid(300, 200), Some(128), Some(128), ResizeMode::Cover);
        assert_eq!(sniff(&bytes), Some(ImageFormat::Png));
        validate(&Bytes::from(bytes), "image/png", &UploadLimits::default())
            .expect("a thumbnail must itself pass upload validation");
    }

    #[test]
    fn jpeg_and_webp_output_are_reachable_and_sniffable() {
        for (format, expected) in [
            (OutputFormat::Jpeg, ImageFormat::Jpeg),
            (OutputFormat::Webp, ImageFormat::Webp),
        ] {
            let bytes = resize_local(ResizeRequest {
                source: &solid(64, 64),
                source_format: ImageFormat::Png,
                width: Some(32),
                height: Some(32),
                mode: ResizeMode::Inside,
                format,
                quality: 85,
            })
            .expect("resize");
            assert_eq!(sniff(&bytes), Some(expected), "{format:?}");
            assert!(format.content_type().starts_with("image/"));
        }
    }

    #[test]
    fn avif_is_refused_loudly_rather_than_substituted() {
        let err = resize_local(ResizeRequest {
            source: &solid(64, 64),
            source_format: ImageFormat::Png,
            width: Some(32),
            height: Some(32),
            mode: ResizeMode::Inside,
            format: OutputFormat::Avif,
            quality: 85,
        })
        .expect_err("avif should be refused");
        assert!(err.message().contains("AVIF"), "{}", err.message());
    }

    #[test]
    fn an_out_of_range_quality_is_refused() {
        for quality in [0u8, 200] {
            let err = resize_local(ResizeRequest {
                source: &solid(64, 64),
                source_format: ImageFormat::Png,
                width: Some(32),
                height: Some(32),
                mode: ResizeMode::Inside,
                format: OutputFormat::Png,
                quality,
            })
            .expect_err("quality should be refused");
            assert!(err.message().contains("1-100"), "{}", err.message());
        }
    }

    #[test]
    fn garbage_is_refused_without_panicking() {
        let err = resize_local(ResizeRequest {
            source: b"not an image at all",
            source_format: ImageFormat::Png,
            width: Some(32),
            height: Some(32),
            mode: ResizeMode::Inside,
            format: OutputFormat::Png,
            quality: 80,
        })
        .expect_err("garbage should be refused");
        assert!(err.message().contains("decode"), "{}", err.message());
    }

    #[test]
    fn output_formats_round_trip_and_the_source_decides_the_default() {
        for name in ["png", "jpeg", "webp", "avif"] {
            assert_eq!(OutputFormat::parse(name).expect("parse").as_str(), name);
        }
        assert_eq!(
            OutputFormat::parse("jpg").expect("parse"),
            OutputFormat::Jpeg
        );
        assert!(OutputFormat::parse("bmp").is_err());

        assert_eq!(
            OutputFormat::from_source(ImageFormat::Jpeg),
            OutputFormat::Jpeg
        );
        assert_eq!(
            OutputFormat::from_source(ImageFormat::Webp),
            OutputFormat::Webp
        );
        // A PNG stays PNG, and a GIF frame becomes PNG so transparency survives.
        assert_eq!(
            OutputFormat::from_source(ImageFormat::Png),
            OutputFormat::Png
        );
        assert_eq!(
            OutputFormat::from_source(ImageFormat::Gif),
            OutputFormat::Png
        );
    }
}

/// A build with `--no-default-features`, which carries no image codecs at all.
#[cfg(all(test, not(feature = "local-image-codecs")))]
mod without_codecs {
    use super::*;

    #[tokio::test]
    async fn a_local_resize_names_the_two_ways_out() {
        let request = ResizeRequest {
            source: b"not an image",
            source_format: ImageFormat::Png,
            width: Some(64),
            height: Some(64),
            mode: ResizeMode::Cover,
            format: OutputFormat::Webp,
            quality: 80,
        };

        let err = resize(request, None).await.expect_err("no codecs");
        assert!(
            err.message().contains("LAZYPOCK_IMAGE_SERVICE_URL"),
            "{}",
            err.message()
        );
        assert!(
            err.message().contains("local-image-codecs"),
            "{}",
            err.message()
        );
    }
}
