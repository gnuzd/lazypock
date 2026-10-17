//! First-run setup: whether a superuser exists, and creating the first one.
//!
//! Both endpoints are **public**, and they have to be: a client calls them before
//! anybody can log in, to decide between the login screen and a setup screen. There
//! is nobody to authenticate as yet, so authentication cannot be the guard.
//!
//! What makes them safe is that `setup` **only works while no superuser exists**. A
//! client already sitting on the endpoint could otherwise create itself a superuser
//! on a running installation, which is a complete takeover. The check and the insert
//! share a transaction holding an advisory lock, so two concurrent requests on a
//! fresh installation cannot both win and create an account each.

use axum::extract::{Json, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lazypock_auth::superuser::SETUP_LOCK;
use lazypock_core::error::Error;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::ApiError;
use crate::state::AppState;

/// The shortest password setup accepts.
///
/// Setup is where the very first credential of an installation is chosen, and it is
/// chosen once; a weak one there is the weakest link for everything behind it.
const MIN_PASSWORD: usize = 8;

/// The body setup accepts. The field is `email`, not the `identity` a login sends.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetupRequest {
    /// The superuser's email address.
    pub email: String,
    /// The password, at least [`MIN_PASSWORD`] characters.
    pub password: String,
}

/// `GET /api/superusers/check`
///
/// Whether the installation has been initialised. It discloses only that, which a
/// login screen reveals anyway by existing.
pub async fn check(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let client = state.db.client().await.map_err(ApiError::new)?;
    let count = lazypock_auth::superuser::count(&client)
        .await
        .map_err(ApiError::new)?;
    Ok(Json(json!({ "has_superuser": count > 0 })))
}

/// `POST /api/superusers/setup`
///
/// Creates the first superuser and returns a token, so the caller is logged in
/// straight after setting up — matching what the client expects.
pub async fn setup(
    State(state): State<AppState>,
    body: Result<Json<SetupRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(request) = crate::auth::json_body(body)?;

    let email = request.email.trim().to_string();
    if email.is_empty() || !email.contains('@') {
        return Err(ApiError::new(Error::invalid(
            "a valid email address is required",
        )));
    }
    if request.password.chars().count() < MIN_PASSWORD {
        return Err(ApiError::new(Error::invalid(format!(
            "the password must be at least {MIN_PASSWORD} characters"
        ))));
    }

    let mut client = state.db.client().await.map_err(ApiError::new)?;
    let transaction = client
        .transaction()
        .await
        .map_err(|err| ApiError::new(lazypock_db::pool::db_error(err)))?;

    // Serialise setup attempts before counting, so two requests arriving together on
    // a fresh installation cannot both see zero and both create an account.
    transaction
        .execute("select pg_advisory_xact_lock($1::bigint)", &[&SETUP_LOCK])
        .await
        .map_err(|err| ApiError::new(lazypock_db::pool::db_error(err)))?;

    let existing = lazypock_auth::superuser::count(&transaction)
        .await
        .map_err(ApiError::new)?;
    if existing > 0 {
        return Err(ApiError::new(Error::forbidden(
            "this installation already has a superuser; log in instead of setting up",
        )));
    }

    lazypock_auth::superuser::create(&transaction, &email, &request.password)
        .await
        .map_err(ApiError::new)?;
    transaction
        .commit()
        .await
        .map_err(|err| ApiError::new(lazypock_db::pool::db_error(err)))?;

    tracing::info!(email = %email, "created the first superuser via setup");

    // Then log in through the same path a normal login takes, so setup cannot return
    // a token that login would refuse.
    let outcome = state
        .auth
        .authenticate_superuser(&email, &request.password)
        .await?;
    let record = json!({
        "id": outcome.user_id,
        "email": email,
        "collectionName": crate::auth::SUPERUSERS,
    });

    Ok((
        StatusCode::CREATED,
        Json(crate::auth::auth_body(&outcome, record)),
    )
        .into_response())
}
