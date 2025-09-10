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
    #[error("Internal Error: `{0}`")]
    Internal(String),
    #[error("Database error: `{0}`")]
    Database(#[from] sqlx::Error),
    #[error("IO error: `{0}`")]
    IO(#[from] std::io::Error),
    #[error(transparent)]
    AnyError(#[from] Box<dyn Error + Send + Sync>),
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        if !matches!(
            self,
            HttpError::BadRequest(_) | HttpError::NotFound | HttpError::Unauthorized
        ) {
            if let Some(source) = self.source() {
                error!("Error: {self}, caused by: {source}");
            } else {
                error!("Error: {self}");
            }
        }

        match self {
            HttpError::BadRequest(message) => (StatusCode::BAD_REQUEST, message).into_response(),
            HttpError::NotFound => StatusCode::NOT_FOUND.into_response(),
            HttpError::Unauthorized => StatusCode::UNAUTHORIZED.into_response(),
            HttpError::Internal(message) => {
                (StatusCode::INTERNAL_SERVER_ERROR, message).into_response()
            }
            HttpError::Database(error) => {
                (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
            }
            HttpError::IO(error) => {
                (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
            }
            HttpError::AnyError(error) => {
                (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
            }
        }
    }
}

pub type HttpResult<T = Response> = Result<T, HttpError>;
