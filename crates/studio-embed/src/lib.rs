//! The Studio SPA, embedded in the server binary.
//!
//! The bundle is produced by `studio/` and placed in `dist/` by `build.rs`; see
//! that file for where it comes from. Nothing here knows how the SPA was built,
//! only how to resolve a request path to bytes, a media type and a cache policy.
//!
//! A build with no bundle is supported: [`is_embedded`] returns `false` and the
//! caller is expected to say so rather than serve an empty page.

use std::borrow::Cow;

use rust_embed::RustEmbed;

/// Where the Studio is mounted, matching `studio/svelte.config.js`'s
/// `paths.base`.
pub const MOUNT: &str = "/_";

/// The SvelteKit build output, placed here by `build.rs`.
#[derive(RustEmbed)]
#[folder = "dist"]
struct Assets;

/// How long a response may be cached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cache {
    /// The file name carries a content hash, so the bytes at that name can
    /// never change and the response may be cached indefinitely.
    Immutable,
    /// The entry point and small metadata files: revalidate every time, or a
    /// deploy would keep serving the previous build's asset names.
    Revalidate,
}

/// One asset to serve.
pub struct Asset {
    /// The file's bytes.
    pub bytes: Cow<'static, [u8]>,
    /// The `Content-Type` to send.
    pub content_type: &'static str,
    /// How the response may be cached.
    pub cache: Cache,
}

/// Whether this build carries a Studio bundle.
pub fn is_embedded() -> bool {
    Assets::get("index.html").is_some()
}

/// The entry point, when the build has one.
pub fn index() -> Option<Asset> {
    asset_for("index.html")
}

/// Every embedded path. Empty when the build has no bundle.
pub fn paths() -> Vec<String> {
    Assets::iter().map(|path| path.to_string()).collect()
}

/// Resolve a request path, relative to [`MOUNT`], to an asset.
///
/// A path that names a file but does not exist is `None`: a browser asking for
/// a missing `.js` has to get a 404, because answering it with the entry point
/// turns an obvious 404 into a syntax error on an HTML body. A path with no
/// extension is a client-side route, so it falls back to the entry point.
pub fn resolve(path: &str) -> Option<Asset> {
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        return index();
    }
    if let Some(asset) = asset_for(path) {
        return Some(asset);
    }
    if looks_like_a_file(path) {
        return None;
    }
    index()
}

/// Whether the last path segment carries an extension.
fn looks_like_a_file(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|last| last.contains('.') && !last.starts_with('.'))
}

/// Look up a file and describe how to serve it.
fn asset_for(path: &str) -> Option<Asset> {
    let file = Assets::get(path)?;
    Some(Asset {
        bytes: file.data,
        content_type: content_type(path),
        cache: cache_policy(path),
    })
}

/// Hashed assets may be cached forever; everything else must be revalidated.
fn cache_policy(path: &str) -> Cache {
    if path.starts_with("_app/immutable/") {
        Cache::Immutable
    } else {
        Cache::Revalidate
    }
}

/// The media type for a path, by extension.
///
/// Written out rather than taken from a crate: a SvelteKit build emits a
/// bounded set of types, and an extension we do not know is better served as
/// `application/octet-stream` than guessed at.
fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map" | "webmanifest") => "application/json",
        Some("txt") => "text/plain; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A missing file must not answer with the entry point, in any build.
    #[test]
    fn a_path_that_names_a_missing_file_does_not_fall_back() {
        assert!(resolve("_app/immutable/entry/does-not-exist.js").is_none());
        assert!(resolve("missing.css").is_none());
    }

    /// A route without an extension is a client-side route, so it falls back to
    /// the entry point — which exists only when a bundle was embedded.
    #[test]
    fn a_route_without_an_extension_falls_back_to_the_entry_point() {
        let route = resolve("collections/posts");
        assert_eq!(route.is_some(), is_embedded());
        if let Some(asset) = route {
            assert_eq!(asset.content_type, "text/html; charset=utf-8");
            assert_eq!(asset.cache, Cache::Revalidate);
        }
    }

    /// The mount root is the entry point.
    #[test]
    fn the_empty_path_is_the_entry_point() {
        assert_eq!(resolve("/").is_some(), is_embedded());
        assert_eq!(resolve("/").is_some(), index().is_some());
    }

    #[test]
    fn the_entry_point_is_html_that_revalidates() {
        let Some(asset) = index() else {
            return;
        };
        assert_eq!(asset.content_type, "text/html; charset=utf-8");
        assert_eq!(asset.cache, Cache::Revalidate);
        assert!(!asset.bytes.is_empty());
    }

    /// Whatever the bundle happens to contain, the hashed directory is
    /// immutable and everything else revalidates.
    #[test]
    fn the_cache_policy_follows_the_hashed_directory() {
        for path in paths() {
            let asset = asset_for(&path).unwrap_or_else(|| panic!("{path} does not resolve"));
            let expected = if path.starts_with("_app/immutable/") {
                Cache::Immutable
            } else {
                Cache::Revalidate
            };
            assert_eq!(asset.cache, expected, "{path}");
        }
    }

    #[test]
    fn media_types_come_from_the_extension() {
        assert_eq!(content_type("index.html"), "text/html; charset=utf-8");
        assert_eq!(
            content_type("a/_app/x.js"),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(content_type("style.css"), "text/css; charset=utf-8");
        assert_eq!(content_type("logo.svg"), "image/svg+xml");
        assert_eq!(content_type("font.woff2"), "font/woff2");
        assert_eq!(content_type("no-extension"), "application/octet-stream");
        assert_eq!(content_type("strange.xyz"), "application/octet-stream");
    }
}
