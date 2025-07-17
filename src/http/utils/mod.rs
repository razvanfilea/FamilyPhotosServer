use axum::body::Body;
use axum::extract::multipart;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use tokio::fs;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio_util::io::ReaderStream;
use tracing::error;

use crate::repo::users_repo::UsersRepository;
use crate::utils::internal_error;

pub type AxumResult<T> = axum::response::Result<T>;

pub type AuthSession = axum_login::AuthSession<UsersRepository>;

pub async fn file_to_response(
    photo_path: &std::path::Path,
) -> AxumResult<impl IntoResponse + use<>> {
    let mime = mime_guess::from_path(photo_path)
        .first_or_octet_stream()
        .as_ref()
        .to_string();

    let stream = ReaderStream::new(fs::File::open(&photo_path).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to open file: {e}"),
        )
    })?);
    // convert the `Stream` into an `axum::body::HttpBody`
    let body = Body::from_stream(stream);

    let headers = [
        (header::CONTENT_TYPE, mime),
        (
            header::CONTENT_DISPOSITION,
            format!(
                "attachment; filename=\"{}\"",
                photo_path
                    .file_name()
                    .expect("Photo must have a name")
                    .to_string_lossy()
            ),
        ),
    ];

    Ok((headers, body))
}

///
/// Returns the number of bytes written to disk
///
pub async fn write_field_to_file<'a, 'b>(
    mut field: multipart::Field<'a>,
    file_path: &'b std::path::Path,
) -> AxumResult<usize> {
    let file = fs::File::create(file_path).await.map_err(|e| {
        error!("Failed creating photo file: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed creating photo file",
        )
    })?;

    let mut writer = BufWriter::new(file);
    let mut written_bytes = 0;

    while let Some(chunk) = field.chunk().await? {
        written_bytes += chunk.len();
        writer.write_all(&chunk).await.map_err(internal_error)?;
    }

    Ok(written_bytes)
}
