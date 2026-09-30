use axum::{
    Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use std::io;
use std::time::Duration;
use tracing::error;

use crate::http::error::{HttpError, HttpResult};
use crate::http::utils::{
    CACHE_IMMUTABLE, CACHE_NO_STORE, CACHE_RENDITION, FileResponseOptions, file_to_response,
};
use crate::http::{AppStateRef, auth::AuthenticatedUser};
use crate::model::photo::PhotoWithFolder;
use crate::previews::{Rendition, rendition_path};
use crate::repo::{PhotoAccess, PhotosRepo};
use crate::utils::media::{is_gif_or_webp, is_video};

pub fn router() -> Router<AppStateRef> {
    Router::new()
        .route("/download/{photo_id}", get(download_photo))
        .route("/preview/{photo_id}", get(preview_photo))
        .route("/screen/{photo_id}", get(screen_photo))
}

/// Seconds after which the UI should retry a preview that couldn't be generated
const RETRY_AFTER_SECS: &str = "5";

fn rendition_unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [
            (
                header::RETRY_AFTER,
                HeaderValue::from_static(RETRY_AFTER_SECS),
            ),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
    )
        .into_response()
}

async fn get_readable_photo(
    state: AppStateRef,
    photo_id: i64,
    user_id: &str,
) -> HttpResult<PhotoWithFolder> {
    state
        .read_pool
        .get_accessible_photo_with_folder(photo_id, user_id, PhotoAccess::Read)
        .await?
        .ok_or(HttpError::NotFound)
}

/// How long a preview request waits for the generation before falling back to the original
const PREVIEW_WAIT: Duration = Duration::from_secs(3);

/// Generates a rendition if needed. `false` (after logging) if it couldn't be generated.
async fn ensure_rendition(
    state: AppStateRef,
    pf: &PhotoWithFolder,
    rendition: Rendition,
    wait: Option<Duration>,
) -> bool {
    let photo_path = state.storage.resolve_photo(pf.partial_path());
    let path = rendition_path(&state.storage, rendition, pf.photo.id);
    let generation = state
        .previews
        .ensure(rendition, pf.photo.id, &photo_path, &path);

    let result = match wait {
        Some(wait) => tokio::time::timeout(wait, generation)
            .await
            .unwrap_or_else(|_| Err(io::Error::new(io::ErrorKind::TimedOut, "still generating"))),
        None => generation.await,
    };

    match result {
        Ok(()) => true,
        Err(e) => {
            error!(
                "{rendition:?} generation failed for: {}\nCause: {e}",
                photo_path.display()
            );
            false
        }
    }
}

async fn rendition_response(
    state: AppStateRef,
    pf: &PhotoWithFolder,
    rendition: Rendition,
    method: &Method,
    headers: &HeaderMap,
) -> HttpResult<Response> {
    file_to_response(
        &rendition_path(&state.storage, rendition, pf.photo.id),
        method,
        headers,
        FileResponseOptions {
            cache_control: CACHE_RENDITION,
            inline: true,
            file_name: None,
            validators: true,
        },
    )
    .await
}

/// 320px webp thumbnail. If it can't be generated (in time), the original is served instead, like
/// before the web UI rewrite (the Android app relies on it). The fallback is `no-store` and has no
/// validators, so the real preview replaces it on the next request.
async fn preview_photo(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    AuthenticatedUser(user): AuthenticatedUser,
    method: Method,
    headers: HeaderMap,
) -> HttpResult<Response> {
    let pf = get_readable_photo(state, photo_id, &user.id).await?;

    if ensure_rendition(state, &pf, Rendition::Preview, Some(PREVIEW_WAIT)).await {
        return rendition_response(state, &pf, Rendition::Preview, &method, &headers).await;
    }

    file_to_response(
        &state.storage.resolve_photo(pf.partial_path()),
        &method,
        &headers,
        FileResponseOptions {
            cache_control: CACHE_NO_STORE,
            inline: true,
            file_name: Some(&pf.photo.name),
            validators: false,
        },
    )
    .await
}

/// ~2048px webp for the viewer. Images only; GIFs and WebPs are served as they are (they may be
/// animated, and a rendition would keep only the first frame).
async fn screen_photo(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    AuthenticatedUser(user): AuthenticatedUser,
    method: Method,
    headers: HeaderMap,
) -> HttpResult<Response> {
    let pf = get_readable_photo(state, photo_id, &user.id).await?;
    if is_video(&pf.photo.name) {
        return Err(HttpError::NotFound);
    }

    let photo_path = state.storage.resolve_photo(pf.partial_path());
    if is_gif_or_webp(&pf.photo.name) {
        return file_to_response(
            &photo_path,
            &method,
            &headers,
            FileResponseOptions {
                cache_control: CACHE_RENDITION,
                inline: true,
                file_name: Some(&pf.photo.name),
                validators: true,
            },
        )
        .await;
    }

    if !ensure_rendition(state, &pf, Rendition::Screen, None).await {
        return Ok(rendition_unavailable());
    }
    rendition_response(state, &pf, Rendition::Screen, &method, &headers).await
}

#[derive(Deserialize)]
struct DownloadQuery {
    /// `?inline=1` displays the original in the browser instead of downloading it
    inline: Option<String>,
}

async fn download_photo(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    Query(query): Query<DownloadQuery>,
    AuthenticatedUser(user): AuthenticatedUser,
    method: Method,
    headers: HeaderMap,
) -> HttpResult<Response> {
    let pf = get_readable_photo(state, photo_id, &user.id).await?;
    let photo_path = state.storage.resolve_photo(pf.partial_path());
    let inline = matches!(query.inline.as_deref(), Some("1" | "true"));

    file_to_response(
        &photo_path,
        &method,
        &headers,
        FileResponseOptions {
            cache_control: CACHE_IMMUTABLE,
            inline,
            file_name: Some(&pf.photo.name),
            validators: true,
        },
    )
    .await
}
