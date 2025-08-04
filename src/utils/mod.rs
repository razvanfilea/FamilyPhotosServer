use axum::http::StatusCode;
use axum::response::{ErrorResponse, IntoResponse};

pub mod env_reader;
pub mod exif;
pub mod password_hash;
pub mod storage_resolver;

/// Utility function for mapping any error into a `500 Internal Server Error`
/// response.
pub fn internal_error<E>(err: E) -> ErrorResponse
where
    E: std::error::Error,
{
    ErrorResponse::from((StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response())
}

pub fn crop_sha_256(hash: &[u8; 32]) -> Vec<u8> {
    hash[..16].to_vec()
}
