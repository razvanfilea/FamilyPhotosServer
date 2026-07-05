use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use std::time::Duration;
use tokio::task;
use tracing::error;

use crate::http::error::{HttpError, HttpResult};
use crate::http::utils::file_to_response;
use crate::http::{AppStateRef, auth::AuthenticatedUser};
use crate::previews;
use crate::repo::{PhotoAccess, PhotosRepo};
use crate::utils::exif::read_exif;
use axum_extra::TypedHeader;
use axum_extra::headers::Range;

pub fn router() -> Router<AppStateRef> {
    Router::new()
        .route("/download/{photo_id}", get(download_photo))
        .route("/preview/{photo_id}", get(preview_photo))
        .route("/exif/{photo_id}", get(get_photo_exif))
}

async fn preview_photo(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    range: Option<TypedHeader<Range>>,
    AuthenticatedUser(user): AuthenticatedUser,
) -> HttpResult<impl IntoResponse> {
    let storage = &state.storage;

    let pf = state
        .read_pool
        .get_accessible_photo_with_folder(photo_id, &user.id, PhotoAccess::Read)
        .await?
        .ok_or(HttpError::NotFound)?;

    let photo_path = storage.resolve_photo(pf.partial_path());
    let preview_path = storage.resolve_preview(pf.partial_preview_path());

    let preview_generation_mutex =
        tokio::time::timeout(Duration::from_secs(3), state.preview_generation.lock()).await;
    if preview_generation_mutex.is_err() {
        return file_to_response(&photo_path, range).await;
    }

    let needs_generation = match tokio::fs::metadata(&preview_path).await {
        Ok(m) => m.len() < previews::MIN_PREVIEW_SIZE,
        Err(_) => true,
    };

    let preview_generated = if needs_generation {
        let photo_path_clone = photo_path.clone();
        let preview_path_clone = preview_path.clone();

        task::spawn_blocking(move || {
            previews::generate_preview(photo_path_clone, preview_path_clone)
        })
        .await
        .map_err(|e| HttpError::AnyError(Box::new(e)))?
    } else {
        Ok(())
    };

    let path = match preview_generated {
        Ok(_) => preview_path,
        Err(e) => {
            error!(
                "Preview generation failed for: {}\nCause: {e}",
                photo_path.display()
            );
            photo_path
        }
    };

    file_to_response(&path, range).await
}

async fn download_photo(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    range: Option<TypedHeader<Range>>,
    AuthenticatedUser(user): AuthenticatedUser,
) -> HttpResult<impl IntoResponse> {
    let pf = state
        .read_pool
        .get_accessible_photo_with_folder(photo_id, &user.id, PhotoAccess::Read)
        .await?
        .ok_or(HttpError::NotFound)?;

    let photo_path = state.storage.resolve_photo(pf.partial_path());

    file_to_response(&photo_path, range).await
}

async fn get_photo_exif(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    AuthenticatedUser(user): AuthenticatedUser,
) -> HttpResult<impl IntoResponse> {
    let pf = state
        .read_pool
        .get_accessible_photo_with_folder(photo_id, &user.id, PhotoAccess::Read)
        .await?
        .ok_or(HttpError::NotFound)?;

    let path = state.storage.resolve_photo(pf.partial_path());
    let exif = task::spawn_blocking(move || read_exif(path))
        .await
        .map_err(|e| HttpError::AnyError(Box::new(e)))?;

    match exif {
        Some(exif) => Ok(Json(exif).into_response()),
        None => Ok((StatusCode::NOT_FOUND, "Exif data not found").into_response()),
    }
}
