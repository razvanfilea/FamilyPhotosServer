use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use std::error::Error;
use thiserror::Error;
use tracing::error;

#[derive(Error, Debug)]
pub enum HttpError {
    #[error("BadRequest Status: `{0}`")]
    BadRequest(String),
    #[error("NotFound Status")]
    NotFound,
    #[error("Unauthorized Status")]
    Unauthorized,
    /// 409 with a user-facing message
    #[error("Conflict Status: `{0}`")]
    Conflict(String),
    #[error("Internal Error: `{0}`")]
    Internal(String),
    #[error("Database error: `{0}`")]
    Database(#[from] sqlx::Error),
    #[error("IO error: `{0}`")]
    IO(#[from] std::io::Error),
    #[error("Failed to generate HTML: `{0}`")]
    Template(#[from] askama::Error),
    #[error(transparent)]
    AnyError(#[from] Box<dyn Error + Send + Sync>),
}

/// The body of every 500 response. The real error is only logged, so that database or file
/// system details don't leak to clients.
pub const INTERNAL_ERROR_MESSAGE: &str = "Internal server error";

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        match self {
            HttpError::BadRequest(message) => (StatusCode::BAD_REQUEST, message).into_response(),
            HttpError::NotFound => StatusCode::NOT_FOUND.into_response(),
            HttpError::Unauthorized => StatusCode::UNAUTHORIZED.into_response(),
            HttpError::Conflict(message) => (StatusCode::CONFLICT, message).into_response(),
            error => {
                if let Some(source) = error.source() {
                    error!("Error: {error}, caused by: {source}");
                } else {
                    error!("Error: {error}");
                }
                (StatusCode::INTERNAL_SERVER_ERROR, INTERNAL_ERROR_MESSAGE).into_response()
            }
        }
    }
}

/// Whether the error is a violation of a unique index (e.g. a duplicate folder name)
pub fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(e) if e.is_unique_violation())
}

pub type HttpResult<T = Response> = Result<T, HttpError>;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn body(error: HttpError) -> (StatusCode, String) {
        let response = error.into_response();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn test_error_bodies() {
        assert_eq!(
            body(HttpError::BadRequest("Bad name".into())).await,
            (StatusCode::BAD_REQUEST, "Bad name".into())
        );
        assert_eq!(
            body(HttpError::Conflict("Exists".into())).await,
            (StatusCode::CONFLICT, "Exists".into())
        );
        assert_eq!(
            body(HttpError::Database(sqlx::Error::RowNotFound)).await,
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                INTERNAL_ERROR_MESSAGE.into()
            )
        );
        assert_eq!(
            body(HttpError::Internal("secret details".into())).await,
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                INTERNAL_ERROR_MESSAGE.into()
            )
        );
    }
}
