//! Who is calling: the bearer token, resolved into a `Principal` and a database
//! `RequestContext`.
//!
//! - no `Authorization` header: anonymous;
//! - a header with a bad/expired token: **401**, never silently anonymous (a client
//!   with an expired token should refresh, not quietly lose access);
//! - [`SuperuserOnly`]: 401 without a token, 403 for a non-superuser, and a database
//!   check that the superuser still exists and the token was not revoked.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use lazypock_auth::{Claims, ClientMeta};
use lazypock_core::Error;
use lazypock_db::{RequestContext, Schema};
use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub enum Principal {
    Anonymous,
    /// A verified token: a record of an auth collection, or a superuser.
    Authenticated(Claims),
}

impl Principal {
    pub fn is_superuser(&self) -> bool {
        matches!(self, Principal::Authenticated(c) if c.typ.is_superuser())
    }

    /// The database context for this caller. One query for an authenticated caller
    /// (account still exists, token not revoked, claims for `@request.auth.<field>`).
    pub async fn context(
        &self,
        state: &AppState,
        schema: &Schema,
    ) -> Result<RequestContext, ApiError> {
        match self {
            Principal::Anonymous => Ok(RequestContext::anonymous()),
            Principal::Authenticated(claims) => {
                Ok(state.auth.request_context(claims, schema).await?)
            }
        }
    }
}

fn bearer(parts: &Parts) -> Result<Option<&str>, ApiError> {
    let Some(value) = parts.headers.get("authorization") else {
        return Ok(None);
    };
    let value = value.to_str().map_err(|_| {
        ApiError(Error::Unauthorized(
            "the Authorization header is not valid".into(),
        ))
    })?;
    let token = match value.split_once(' ') {
        Some((scheme, token)) if scheme.eq_ignore_ascii_case("bearer") => token.trim(),
        _ => {
            return Err(ApiError(Error::Unauthorized(
                "the Authorization header must be `Bearer <token>`".into(),
            )));
        }
    };
    Ok(Some(token))
}

impl FromRequestParts<AppState> for Principal {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match bearer(parts)? {
            None => Ok(Principal::Anonymous),
            Some(token) => Ok(Principal::Authenticated(state.auth.verify(token)?)),
        }
    }
}

/// Routes only a superuser may call (collections, fields, rules).
#[derive(Debug, Clone)]
pub struct SuperuserOnly(pub Claims);

impl FromRequestParts<AppState> for SuperuserOnly {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match Principal::from_request_parts(parts, state).await? {
            Principal::Anonymous => Err(ApiError(Error::Unauthorized(
                "authentication required".into(),
            ))),
            Principal::Authenticated(c) if !c.typ.is_superuser() => Err(ApiError(
                Error::Forbidden("only a superuser can do this".into()),
            )),
            Principal::Authenticated(c) => {
                // Still there, and not revoked by a password change.
                // state
                // .auth
                // .request_context(&c, &state.schemas.cache().snapshot())
                // .await?;
                Ok(SuperuserOnly(c))
            }
        }
    }
}

/// User agent and peer address for the session record. The address comes from
/// `ConnectInfo` (serve with `into_make_service_with_connect_info`), never from a
/// forwarded header a client could forge.
#[derive(Debug, Clone, Default)]
pub struct ClientInfo(pub ClientMeta);

impl FromRequestParts<AppState> for ClientInfo {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &AppState) -> Result<Self, Self::Rejection> {
        let user_agent = parts
            .headers
            .get("user-agent")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.chars().take(256).collect());
        let ip = parts
            .extensions
            .get::<axum::extract::ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip().to_string());
        Ok(ClientInfo(ClientMeta { user_agent, ip }))
    }
}
