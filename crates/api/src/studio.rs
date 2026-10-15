//! Serving the embedded Studio SPA.
//!
//! The bundle itself is embedded by `lazypock-studio-embed`; this module is the
//! HTTP policy on top of it — which cache header to send, and what to say when a
//! build carries no bundle at all.
//!
//! The routes sit outside `/api/`: the request logger only records `/api/*`, and
//! a browser fetching the page or one of its assets has no credentials to send.
//! `studio/svelte.config.js` sets `paths.base = "/_"`, so the build's own asset
//! URLs already point here.

use axum::body::Body;
use axum::extract::Path;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

use lazypock_core::error::Error;
use lazypock_studio_embed as studio;

use crate::error::ApiError;

/// Where the SPA's entry point lives, as a browser addresses it.
pub const INDEX_PATH: &str = "/_/";

/// Hashed assets may be cached for a year: the name contains a content hash, so
/// a new build produces new names rather than new content at an old name.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// The entry point must be revalidated, or a deploy would keep serving the
/// previous build's asset names and fail to find them.
const REVALIDATE: &str = "no-cache";

/// `GET /` — a browser asking for the deployment's root wants the admin UI.
///
/// The reference implementation serves the Studio here. A 404 at the bare domain
/// would be a poor first impression, and `/` carries no other meaning in this
/// API, so it redirects instead.
pub async fn root() -> Response {
    (
        StatusCode::TEMPORARY_REDIRECT,
        [(header::LOCATION, INDEX_PATH)],
    )
        .into_response()
}

/// `GET /_` and `GET /_/` — the SPA's entry point.
pub async fn index() -> Response {
    match studio::index() {
        Some(asset) => serve(asset),
        None => not_built(),
    }
}

/// `GET /_/{*path}` — an asset, or a client-side route.
pub async fn asset(Path(path): Path<String>) -> Response {
    match studio::resolve(&path) {
        Some(asset) => serve(asset),
        // A bundle is embedded and this file is not in it: a real 404. Answering
        // with the entry point instead would turn a missing `.js` into a syntax
        // error on an HTML body. A path that is *not* a file already fell back
        // to the entry point inside `resolve`.
        None if studio::is_embedded() => ApiError::new(Error::not_found(format!(
            "The requested file was not found: {path}"
        )))
        .into_response(),
        None => not_built(),
    }
}

/// Serve one asset with its media type and cache policy.
fn serve(asset: studio::Asset) -> Response {
    let cache = match asset.cache {
        studio::Cache::Immutable => IMMUTABLE,
        studio::Cache::Revalidate => REVALIDATE,
    };
    (
        [
            (header::CONTENT_TYPE, asset.content_type),
            (header::CACHE_CONTROL, cache),
        ],
        Body::from(asset.bytes.into_owned()),
    )
        .into_response()
}

/// What a build with no Studio answers.
///
/// Deliberately not a 404: the route exists, the bundle does not, and whoever
/// hit it needs to know how to put one there. It renders the same JSON envelope
/// as every other endpoint.
fn not_built() -> Response {
    ApiError::new(Error::unsupported(
        "This build has no Studio embedded. Run `bun run build` in studio/ and set \
         LAZYPOCK_STUDIO_DIR to its output when building the server, or use a build that \
         includes the Studio.",
    ))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderName, StatusCode};

    /// Read one header as text.
    fn header_value(response: &Response, name: &HeaderName) -> Option<String> {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    }

    /// The entry point is HTML that revalidates, or a clear refusal when this
    /// build has no bundle.
    #[tokio::test]
    async fn the_root_serves_the_entry_point_or_says_there_is_none() {
        let response = index().await;
        if studio::is_embedded() {
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                header_value(&response, &header::CONTENT_TYPE).as_deref(),
                Some("text/html; charset=utf-8")
            );
            assert_eq!(
                header_value(&response, &header::CACHE_CONTROL).as_deref(),
                Some(REVALIDATE)
            );
        } else {
            assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        }
    }

    /// The bare domain hands the browser to the admin UI.
    #[tokio::test]
    async fn the_root_redirects_to_the_entry_point() {
        let response = root().await;
        assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(
            header_value(&response, &header::LOCATION).as_deref(),
            Some(INDEX_PATH)
        );
    }

    /// A path that names a file the bundle does not contain is a 404, never the
    /// entry point.
    #[tokio::test]
    async fn a_missing_file_is_not_answered_with_the_entry_point() {
        let response = asset(Path("_app/immutable/entry/nope.js".to_string())).await;
        let expected = if studio::is_embedded() {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::NOT_IMPLEMENTED
        };
        assert_eq!(response.status(), expected);
    }

    /// A client-side route falls back to the entry point when a bundle is there.
    #[tokio::test]
    async fn a_client_side_route_falls_back() {
        let response = asset(Path("collections/posts".to_string())).await;
        let expected = if studio::is_embedded() {
            StatusCode::OK
        } else {
            StatusCode::NOT_IMPLEMENTED
        };
        assert_eq!(response.status(), expected);
        if studio::is_embedded() {
            assert_eq!(
                header_value(&response, &header::CONTENT_TYPE).as_deref(),
                Some("text/html; charset=utf-8")
            );
        }
    }
}
