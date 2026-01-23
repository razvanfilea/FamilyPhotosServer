use crate::http::error::{HttpError, HttpResult};
use crate::repo::users_repo::UsersRepository;
use crate::utils::crop_blake_3_hash;
use axum::body::Body;
use axum::extract::multipart;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum_extra::TypedHeader;
use axum_extra::headers::Range;
use bytes::Bytes;
use std::io::{BufWriter, SeekFrom, Write};
use std::ops::Bound;
use tempfile::NamedTempFile;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::mpsc;
use tokio_util::io::ReaderStream;

pub type AuthSession = axum_login::AuthSession<UsersRepository>;

pub async fn file_to_response(
    photo_path: &std::path::Path,
    range_header: Option<TypedHeader<Range>>,
) -> HttpResult<Response> {
    let mime = mime_guess::from_path(photo_path)
        .first_or_octet_stream()
        .as_ref()
        .to_string();

    let metadata = fs::metadata(photo_path).await?;
    let file_size = metadata.len();

    let filename = photo_path
        .file_name()
        .expect("Photo must have a name")
        .to_string_lossy();

    // Parse range header if present
    let (start, end, is_range_request) = if let Some(TypedHeader(range)) = range_header {
        if let Some((start_bound, end_bound)) = range.satisfiable_ranges(file_size).next() {
            let start = match start_bound {
                Bound::Included(n) => n,
                Bound::Excluded(n) => n + 1,
                Bound::Unbounded => 0,
            };
            let end = match end_bound {
                Bound::Included(n) => n,
                Bound::Excluded(n) => n.saturating_sub(1),
                Bound::Unbounded => file_size.saturating_sub(1),
            };
            (start, end, true)
        } else {
            // Range not satisfiable
            return Ok((
                StatusCode::RANGE_NOT_SATISFIABLE,
                [(header::CONTENT_RANGE, format!("bytes */{}", file_size))],
            )
                .into_response());
        }
    } else {
        (0, file_size.saturating_sub(1), false)
    };

    let content_length = end - start + 1;

    // Open file and seek to start position
    let mut file = fs::File::open(photo_path).await?;
    if start > 0 {
        file.seek(SeekFrom::Start(start)).await?;
    }

    // Limit reads to content_length bytes
    let limited_reader = file.take(content_length);
    let stream = ReaderStream::new(limited_reader);
    let body = Body::from_stream(stream);

    Ok(if is_range_request {
        (
            StatusCode::PARTIAL_CONTENT,
            [
                (header::CONTENT_TYPE, mime),
                (header::CONTENT_LENGTH, content_length.to_string()),
                (header::ACCEPT_RANGES, "bytes".to_string()),
                (
                    header::CONTENT_RANGE,
                    format!("bytes {}-{}/{}", start, end, file_size),
                ),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}\"", filename),
                ),
            ],
            body,
        )
            .into_response()
    } else {
        (
            [
                (header::CONTENT_TYPE, mime),
                (header::CONTENT_LENGTH, file_size.to_string()),
                (header::ACCEPT_RANGES, "bytes".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}\"", filename),
                ),
            ],
            body,
        )
            .into_response()
    })
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
pub async fn write_field_to_file(mut field: multipart::Field<'_>) -> HttpResult<WrittenFile> {
    let temp_file = NamedTempFile::new()?;
    let (tx, rx) = mpsc::channel::<Bytes>(8);

    let writer_handle = tokio::task::spawn_blocking(move || write_chunks_blocking(temp_file, rx));

    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|e| HttpError::AnyError(Box::new(e)))?
    {
        tx.send(chunk)
            .await
            .map_err(|e| HttpError::AnyError(Box::new(e)))?;
    }

    // Drop sender to signal completion
    drop(tx);

    writer_handle
        .await
        .map_err(|e| HttpError::AnyError(Box::new(e)))?
}

fn write_chunks_blocking(
    mut temp_file: NamedTempFile,
    mut rx: mpsc::Receiver<Bytes>,
) -> HttpResult<WrittenFile> {
    let mut writer = BufWriter::new(temp_file.as_file_mut());
    let mut digest = blake3::Hasher::new();
    let mut written_bytes = 0;

    while let Some(chunk) = rx.blocking_recv() {
        written_bytes += chunk.len();
        writer.write_all(&chunk)?;
        digest.update(&chunk);
    }

    writer.flush()?;
    drop(writer);

    let hash = digest.finalize();

    Ok(WrittenFile {
        temp_file,
        size: written_bytes,
        hash: crop_blake_3_hash(hash.as_bytes()),
    })
}
