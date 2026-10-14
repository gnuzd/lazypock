//! Image preset administration.
//!
//! Reading the list is public, because a client needs the available sizes to
//! build a thumbnail URL. Writing is a superuser action: a preset decides how
//! every image in the installation gets resized, so it is configuration, not
//! content.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use lazypock_core::error::Error;
use lazypock_files::presets::{self, ImagePreset};
use lazypock_files::{OutputFormat, ResizeMode};
use serde::Deserialize;

use crate::auth::Principal;
use crate::error::ApiError;
use crate::state::AppState;

/// `GET /api/image-presets`
pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<ImagePreset>>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let presets = presets::load(&client).await.map_err(ApiError::new)?;
    Ok(Json(presets))
}

/// The body accepted when writing a preset.
///
/// Dimensions are optional individually, matching the schema: a preset may
/// constrain one side only, and the other is derived from the source.
#[derive(Debug, Deserialize)]
pub struct PresetBody {
    /// The name. Required when creating; on update it must match the path.
    #[serde(default)]
    pub name: Option<String>,
    /// Box width.
    #[serde(default)]
    pub width: Option<u32>,
    /// Box height.
    #[serde(default)]
    pub height: Option<u32>,
    /// One of `cover`, `contain`, `fill`, `inside`, `outside`.
    pub fit: String,
    /// One of `png`, `jpeg`, `webp`.
    pub format: String,
    /// Encoder quality, 1-100.
    #[serde(default)]
    pub quality: Option<u8>,
}

impl PresetBody {
    /// Build the preset this body describes.
    fn into_preset(self, name: String) -> Result<ImagePreset, ApiError> {
        Ok(ImagePreset {
            name,
            width: self.width,
            height: self.height,
            mode: ResizeMode::parse(&self.fit).map_err(as_invalid)?,
            format: OutputFormat::parse(&self.format).map_err(as_invalid)?,
            quality: self.quality.unwrap_or(80),
        })
    }
}

/// `POST /api/image-presets`
pub async fn create(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<PresetBody>,
) -> Result<Response, ApiError> {
    require_superuser(&principal)?;

    let name = body
        .name
        .as_deref()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| ApiError::new(Error::invalid("a preset `name` is required")))?
        .to_string();
    let preset = body.into_preset(name)?;

    let client = state.db.client().await.map_err(ApiError::new)?;
    presets::create(&client, &preset)
        .await
        .map_err(ApiError::new)?;

    tracing::info!(name = %preset.name, "image preset created");
    Ok((StatusCode::CREATED, Json(preset)).into_response())
}

/// `PATCH /api/image-presets/{name}`
pub async fn update(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(name): Path<String>,
    Json(body): Json<PresetBody>,
) -> Result<Response, ApiError> {
    require_superuser(&principal)?;

    // The path is authoritative. A body naming something else is a mistake worth
    // reporting rather than a rename to perform, because a rename would orphan
    // every thumbnail already cached under the old name.
    if let Some(body_name) = body.name.as_deref() {
        if body_name != name {
            return Err(ApiError::new(Error::invalid(format!(
                "the body names {body_name:?} but the path names {name:?}; \
                 a preset cannot be renamed"
            ))));
        }
    }

    let preset = body.into_preset(name.clone())?;
    let client = state.db.client().await.map_err(ApiError::new)?;

    if !presets::update(&client, &name, &preset)
        .await
        .map_err(ApiError::new)?
    {
        return Err(ApiError::new(Error::not_found(format!(
            "no image preset named {name:?}"
        ))));
    }

    tracing::info!(name = %name, "image preset updated");
    Ok((StatusCode::OK, Json(preset)).into_response())
}

/// `DELETE /api/image-presets/{name}`
pub async fn delete(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(name): Path<String>,
) -> Result<Response, ApiError> {
    require_superuser(&principal)?;

    let client = state.db.client().await.map_err(ApiError::new)?;
    if !presets::delete(&client, &name)
        .await
        .map_err(ApiError::new)?
    {
        return Err(ApiError::new(Error::not_found(format!(
            "no image preset named {name:?}"
        ))));
    }

    // Thumbnails already generated from this preset stay where they are until the
    // orphan sweeper runs; deleting them here would make this call unbounded.
    tracing::info!(name = %name, "image preset deleted");
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Re-label a parse failure as a bad request.
///
/// `ResizeMode::parse` and `OutputFormat::parse` report a configuration error,
/// which is right when a *stored row* is unreadable and wrong when the value came
/// from a request body — reporting a caller's typo as a 500 would blame the
/// server and hide the real cause.
fn as_invalid(error: Error) -> ApiError {
    ApiError::new(Error::invalid(error.message().to_string()))
}

/// Presets are installation-wide configuration, so only a superuser may change
/// them.
fn require_superuser(principal: &Principal) -> Result<(), ApiError> {
    if principal.is_superuser() {
        Ok(())
    } else {
        Err(ApiError::new(Error::forbidden(
            "managing image presets requires a superuser token",
        )))
    }
}
