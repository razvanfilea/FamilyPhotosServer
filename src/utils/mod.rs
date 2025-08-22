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

const BLAKE_3_LEN: usize = 32;
const HALF_BLAKE_3_LEN: usize = BLAKE_3_LEN / 2;

pub fn crop_blake_3_hash(hash: &[u8; BLAKE_3_LEN]) -> Vec<u8> {
    hash[..HALF_BLAKE_3_LEN].to_vec()
}
