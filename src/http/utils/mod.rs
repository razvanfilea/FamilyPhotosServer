use crate::http::error::{HttpError, HttpResult};
use crate::repo::users_repo::UsersRepository;
use crate::utils::crop_blake_3_hash;
use axum::body::Body;
use axum::extract::FromRequestParts;
use axum::extract::multipart;
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum_extra::headers::{
    ETag, HeaderMapExt, IfModifiedSince, IfNoneMatch, IfRange, LastModified, Range,
};
use std::borrow::Cow;
use std::io::{BufWriter, SeekFrom, Write};
use std::ops::Bound;
use std::time::UNIX_EPOCH;
use tempfile::NamedTempFile;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::mpsc;
use tokio_util::bytes::Bytes;
use tokio_util::io::ReaderStream;

pub type AuthSession = axum_login::AuthSession<UsersRepository>;

/// Extractor that determines if the client wants an HTML response.
/// Returns true for HTMX requests or requests with Accept: text/html.
pub struct WantsHtml(pub bool);

impl<S> FromRequestParts<S> for WantsHtml
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(WantsHtml(wants_html(&parts.headers)))
    }
}

fn wants_html(headers: &HeaderMap) -> bool {
    if headers.contains_key("hx-request") {
        return true;
    }
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("text/html"))
        .unwrap_or(false)
}

/// How a file response should be cached and presented by the browser
#[derive(Debug, Clone, Copy)]
pub struct FileResponseOptions<'a> {
    pub cache_control: &'static str,
    /// `inline` (display in the browser) or `attachment` (download)
    pub inline: bool,
    /// The file name to suggest, defaults to the name of the file on disk
    pub file_name: Option<&'a str>,
    /// Send `ETag`/`Last-Modified` and answer conditional requests. Disable it for responses that
    /// stand in for another resource, so that clients can't revalidate and keep them.
    pub validators: bool,
}

/// Originals never change for a given photo id
pub const CACHE_IMMUTABLE: &str = "private, max-age=31536000, immutable";
/// Previews may be regenerated
pub const CACHE_RENDITION: &str = "private, max-age=604800";
/// Temporary stand-ins, e.g. the original served when its preview couldn't be generated
pub const CACHE_NO_STORE: &str = "no-store";

/// Streams a file, with single-range support. When `options.validators` is set, it also sends a
/// weak `ETag` and `Last-Modified` and answers `If-None-Match` / `If-Modified-Since` with `304`.
/// `HEAD` requests don't open the file.
pub async fn file_to_response(
    path: &std::path::Path,
    method: &Method,
    request_headers: &HeaderMap,
    options: FileResponseOptions<'_>,
) -> HttpResult<Response> {
    let metadata = fs::metadata(path).await?;
    let file_size = metadata.len();
    let modified = metadata.modified().ok();

    let mtime_secs = modified
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    let etag: ETag = format!("W/\"{file_size:x}-{mtime_secs:x}\"")
        .parse()
        .expect("Weak ETag must be valid");
    let last_modified = modified.map(LastModified::from);

    let mut headers = HeaderMap::new();
    if options.validators {
        headers.typed_insert(etag.clone());
        if let Some(last_modified) = last_modified {
            headers.typed_insert(last_modified);
        }
    }
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(options.cache_control),
    );

    let not_modified = options.validators
        && match request_headers.typed_get::<IfNoneMatch>() {
            Some(if_none_match) => !if_none_match.precondition_passes(&etag),
            None => match (request_headers.typed_get::<IfModifiedSince>(), modified) {
                (Some(since), Some(modified)) => !since.is_modified(modified),
                _ => false,
            },
        };
    if not_modified {
        return Ok((StatusCode::NOT_MODIFIED, headers).into_response());
    }

    let file_name = options.file_name.map(Cow::Borrowed).unwrap_or_else(|| {
        path.file_name()
            .expect("File must have a name")
            .to_string_lossy()
    });
    let mime = mime_guess::from_path(file_name.as_ref())
        .first_or_octet_stream()
        .to_string();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&mime)
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&content_disposition(options.inline, &file_name))
            .expect("Content-Disposition must be valid"),
    );

    // A stale If-Range means the client's partial copy is outdated: send the whole file
    let range = request_headers.typed_get::<Range>().filter(|_| {
        request_headers
            .typed_get::<IfRange>()
            .is_none_or(|if_range| {
                options.validators && !if_range.is_modified(Some(&etag), last_modified.as_ref())
            })
    });

    let range = range.map(|range| {
        range
            .satisfiable_ranges(file_size)
            .next()
            .map(|(start_bound, end_bound)| {
                let start = match start_bound {
                    Bound::Included(n) => n,
                    Bound::Excluded(n) => n + 1,
                    Bound::Unbounded => 0,
                };
                let end = match end_bound {
                    Bound::Included(n) => n,
                    Bound::Excluded(n) => n.saturating_sub(1),
                    Bound::Unbounded => file_size.saturating_sub(1),
                }
                .min(file_size.saturating_sub(1));
                (start, end)
            })
            .filter(|(start, end)| start <= end && *start < file_size)
    });

    let (status, start, end) = match range {
        Some(Some((start, end))) => {
            headers.insert(
                header::CONTENT_RANGE,
                HeaderValue::from_str(&format!("bytes {start}-{end}/{file_size}"))
                    .expect("Content-Range must be valid"),
            );
            (StatusCode::PARTIAL_CONTENT, start, end)
        }
        Some(None) => {
            return Ok((
                StatusCode::RANGE_NOT_SATISFIABLE,
                [(header::CONTENT_RANGE, format!("bytes */{file_size}"))],
            )
                .into_response());
        }
        None => (StatusCode::OK, 0, file_size.saturating_sub(1)),
    };

    let content_length = if file_size == 0 { 0 } else { end - start + 1 };
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(content_length));

    if method == Method::HEAD {
        return Ok((status, headers).into_response());
    }

    let mut file = fs::File::open(path).await?;
    if start > 0 {
        file.seek(SeekFrom::Start(start)).await?;
    }
    let body = Body::from_stream(ReaderStream::new(file.take(content_length)));

    Ok((status, headers, body).into_response())
}

/// `inline|attachment; filename="<ascii fallback>"; filename*=UTF-8''<percent-encoded>`
/// (RFC 6266 / RFC 5987)
pub fn content_disposition(inline: bool, file_name: &str) -> String {
    let disposition = if inline { "inline" } else { "attachment" };

    let fallback: String = file_name
        .chars()
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() && c != '"' && c != '\\' {
                c
            } else {
                '_'
            }
        })
        .collect();

    let mut encoded = String::with_capacity(file_name.len());
    for byte in file_name.bytes() {
        if byte.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }

    format!("{disposition}; filename=\"{fallback}\"; filename*=UTF-8''{encoded}")
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

/// The temp file is created in `temp_dir`, which should be on the same filesystem as the
/// final location, so that persisting it is a rename and not a copy.
pub async fn write_field_to_file(
    mut field: multipart::Field<'_>,
    temp_dir: &std::path::Path,
) -> HttpResult<WrittenFile> {
    let temp_file = NamedTempFile::new_in(temp_dir)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_disposition() {
        assert_eq!(
            content_disposition(false, "IMG_1.jpg"),
            "attachment; filename=\"IMG_1.jpg\"; filename*=UTF-8''IMG_1.jpg"
        );
        assert_eq!(
            content_disposition(true, "Plajă \"2024\".jpg"),
            "inline; filename=\"Plaj_ _2024_.jpg\"; filename*=UTF-8''Plaj%C4%83%20%222024%22.jpg"
        );
        assert_eq!(
            content_disposition(true, "a\\b;c\r.png"),
            "inline; filename=\"a_b;c_.png\"; filename*=UTF-8''a%5Cb%3Bc%0D.png"
        );
    }
}
