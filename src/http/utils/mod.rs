use axum::body::Body;
use axum::extract::multipart;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use std::io::{BufWriter, Write};
use tempfile::NamedTempFile;
use tokio::fs;
use tokio_util::io::ReaderStream;
use tracing::error;

use crate::repo::users_repo::UsersRepository;
use crate::utils::{crop_blake_3_hash, internal_error};

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

pub struct WrittenFile {
    temp_file: NamedTempFile,
    pub size: usize,
    pub hash: Vec<u8>,
}

impl WrittenFile {
    /// Moves the temporary file to the target path, handling cross-device scenarios
    pub async fn persist_to(self, target_path: &std::path::Path) -> std::io::Result<()> {
        // First, try the fast path (rename)
        match self.temp_file.persist(target_path) {
            Ok(_) => Ok(()),
            Err(tempfile::PersistError { error, file }) => {
                // If persist failed due to a cross-device link, fall back to copy and delete
                if error.raw_os_error() == Some(18) {
                    // EXDEV: Cross-device link
                    fs::copy(file.path(), target_path).await?;
                    // The temporary file will be automatically cleaned up when dropped
                    Ok(())
                } else {
                    Err(error)
                }
            }
        }
    }
}

///
/// Returns the number of bytes written to disk
///
pub async fn write_field_to_file(mut field: multipart::Field<'_>) -> AxumResult<WrittenFile> {
    let mut temp_file = NamedTempFile::new().map_err(|e| {
        error!("Failed creating photo file: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed creating photo file",
        )
    })?;

    let mut digest = blake3::Hasher::new();

    let mut writer = BufWriter::new(temp_file.as_file_mut());
    let mut written_bytes = 0;

    while let Some(chunk) = field.chunk().await? {
        // TODO Figure out how to make this function async free or async friendly
        written_bytes += chunk.len();
        writer.write_all(&chunk).map_err(internal_error)?;
        digest.update(&chunk);
    }

    writer.flush().map_err(internal_error)?;
    drop(writer);

    let hash = digest.finalize();

    Ok(WrittenFile {
        temp_file,
        size: written_bytes,
        hash: crop_blake_3_hash(hash.as_bytes()),
    })
}
