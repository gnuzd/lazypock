use crate::state::AppState;
use axum::{
    Router,
    body::Body,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::get,
};

#[cfg(not(debug_assertions))]
use rust_embed::RustEmbed;

pub fn config(r: Router<AppState>) -> Router<AppState> {
    r.route("/_", get(studio))
        .route("/_/", get(studio))
        .route("/_{*path}", get(studio))
}

// Only compile RustEmbed in release mode to avoid build errors if studio hasn't been built yet during dev
#[cfg(not(debug_assertions))]
#[derive(RustEmbed)]
#[folder = "../../studio/.nsh/out/studio/"]
struct Assets;

pub async fn studio(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches("/_").trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    // =========================================================================
    // DEV MODE: Read directly from disk without restarting crates/app
    // =========================================================================
    #[cfg(debug_assertions)]
    {
        use std::path::PathBuf;

        // Resolve the absolute path to studio/.nsh/out/studio relative to crates/api
        let base_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../studio/.nsh/out/studio");

        // Sanitize path segments to prevent path traversal attacks
        let mut target_file = base_dir.clone();
        for segment in path.split('/') {
            if !segment.is_empty() && segment != "." && segment != ".." {
                target_file.push(segment);
            }
        }

        // Serve the requested file if it exists, otherwise fall back to index.html for SPA routing
        let (file_to_serve, is_fallback) = if target_file.is_file() {
            (target_file, false)
        } else {
            (base_dir.join("index.html"), true)
        };

        match tokio::fs::read(&file_to_serve).await {
            Ok(bytes) => {
                let mime = if is_fallback {
                    "text/html; charset=utf-8".to_string()
                } else {
                    mime_guess::from_path(&file_to_serve)
                        .first_or_octet_stream()
                        .to_string()
                };

                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, mime)
                    // Disable browser caching in dev mode so updated .js/.wasm bundles load immediately
                    .header(header::CACHE_CONTROL, "no-store, no-cache, must-revalidate")
                    .body(Body::from(bytes))
                    .unwrap()
            }
            Err(_) => StatusCode::NOT_FOUND.into_response(),
        }
    }

    // =========================================================================
    // RELEASE MODE: Serve embedded assets bundled inside the binary
    // =========================================================================
    #[cfg(not(debug_assertions))]
    {
        match Assets::get(path) {
            Some(content) => {
                let mime = mime_guess::from_path(path).first_or_octet_stream();
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, mime.as_ref())
                    .body(Body::from(content.data))
                    .unwrap()
            }
            None => {
                // Fallback to index.html for client-side routing (SPA)
                if let Some(index) = Assets::get("index.html") {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                        .body(Body::from(index.data))
                        .unwrap()
                } else {
                    StatusCode::NOT_FOUND.into_response()
                }
            }
        }
    }
}
