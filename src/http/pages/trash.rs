use crate::http::AppStateRef;
use crate::http::auth::AuthenticatedUser;
use crate::http::error::{HttpError, HttpResult};
use crate::http::pages::gallery::PhotoView;
use crate::http::template_into_response::TemplateIntoResponse;
use crate::repo::{FavoritesRepo, PhotosRepo, PhotosTransactionRepo};
use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse, Response};
use std::collections::HashSet;
use std::io::ErrorKind;
use time::OffsetDateTime;
use tokio::fs;
use tracing::{info, warn};

#[derive(Template)]
#[template(path = "trash/trash_page.html")]
struct TrashPageTemplate {
    photos: Vec<PhotoView>,
}

pub async fn trash_page(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppStateRef>,
) -> HttpResult<Response> {
    // Use optimized query that only fetches trashed photos
    let trashed_photos = state.read_pool.get_trashed_photos(&user.id).await?;
    let photo_ids: Vec<i64> = trashed_photos.iter().map(|p| p.id).collect();

    // Only check favorites for the photos we're displaying
    let favorite_ids: HashSet<i64> = state
        .read_pool
        .check_favorites_for_ids(&user.id, &photo_ids)
        .await?;

    let photos: Vec<PhotoView> = trashed_photos
        .into_iter()
        .map(|p| PhotoView::from_photo(p, &favorite_ids))
        .collect();

    TrashPageTemplate { photos }.try_into_response()
}

pub async fn trash_photo(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
) -> HttpResult<Response> {
    let mut tx = state.write_pool.begin().await?;

    let mut photo = tx
        .get_photo(photo_id, &user.id)
        .await?
        .ok_or(HttpError::NotFound)?;

    photo.trashed_on = Some(OffsetDateTime::now_utc());
    tx.update_photo(&photo).await?;
    tx.commit().await?;

    // Return empty HTML to remove the card via hx-swap="outerHTML"
    Ok(Html("").into_response())
}

pub async fn restore_photo(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
) -> HttpResult<Response> {
    let mut tx = state.write_pool.begin().await?;

    let mut photo = tx
        .get_photo(photo_id, &user.id)
        .await?
        .ok_or(HttpError::NotFound)?;

    photo.trashed_on = None;
    tx.update_photo(&photo).await?;
    tx.commit().await?;

    // Return empty response with HX-Trigger to refresh the page
    Ok([("HX-Refresh", "true")].into_response())
}

pub async fn permanent_delete(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
) -> HttpResult<Response> {
    let mut tx = state.write_pool.begin().await?;

    let photo = tx
        .get_photo(photo_id, &user.id)
        .await?
        .ok_or(HttpError::NotFound)?;

    // First: DB operations in transaction
    tx.delete_photo(&photo).await?;
    tx.commit().await?;

    // After commit succeeds: clean up files
    // Preview file - ignore errors (might not exist)
    let preview_path = state.storage.resolve_preview(photo.partial_preview_path());
    if let Err(e) = fs::remove_file(&preview_path).await
        && e.kind() != ErrorKind::NotFound
    {
        warn!(
            "Failed to delete preview at {}: {}",
            preview_path.display(),
            e
        );
    }

    // Photo file - ignore "not found" (already deleted), log other errors
    let photo_path = state.storage.resolve_photo(photo.partial_path());
    match fs::remove_file(&photo_path).await {
        Ok(()) => info!("Removed file at {}", photo_path.display()),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            // Already deleted, this is fine
        }
        Err(e) => {
            warn!("Failed to delete photo at {}: {}", photo_path.display(), e);
        }
    }

    // Return empty response with HX-Trigger to refresh the page
    Ok([("HX-Refresh", "true")].into_response())
}
