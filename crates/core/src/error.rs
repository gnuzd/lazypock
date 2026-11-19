use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("config error: {0}")]
    Config(String),

    #[error("database error: {0}")]
    Database(String),

    #[error("schema error: {0}")]
    Schema(String),

    #[error("validation error: {0}")]
    Validation(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("internal error: {0}")]
    Internal(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl Error {
    pub fn status_code(&self) -> u16 {
        match self {
            Error::NotFound(_) => 404,
            Error::Validation(_) | Error::BadRequest(_) => 400,
            Error::Conflict(_) => 409,
            Error::Unauthorized(_) => 401,
            Error::Forbidden(_) => 403,
            Error::Config(_)
            | Error::Database(_)
            | Error::Schema(_)
            | Error::Internal(_)
            | Error::Other(_) => 500,
        }
    }
}
