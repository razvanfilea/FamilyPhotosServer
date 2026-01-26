use crate::http::AppStateRef;
use crate::http::error::{HttpError, HttpResult};
use crate::http::pages::gallery::PhotoView;
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::utils::AuthSession;
use crate::repo::{FavoritesRepo, PhotosRepo, PhotosTransactionRepo};
use askama::Template;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use tokio::fs;
use tracing::{info, warn};

#[derive(Template)]
#[template(path = "trash/trash_page.html")]
struct TrashPageTemplate {
    photos: Vec<PhotoView>,
}

pub async fn trash_page(
    auth_session: AuthSession,
    State(state): State<AppStateRef>,
) -> HttpResult<Response> {
    let user = auth_session.user.expect("User must be authenticated");

    let mut tx = state.pool.begin().await?;
    let all_photos = tx.get_photos_by_user_and_public(&user.id).await?.photos;
    let favorite_ids: std::collections::HashSet<i64> = tx
        .get_favorite_photos(&user.id)
        .await?
        .into_iter()
        .collect();
    tx.commit().await?;

    let photos: Vec<PhotoView> = all_photos
        .into_iter()
        .filter(|p| p.trashed_on.is_some())
        .map(|p| PhotoView::from_photo(p, &favorite_ids))
        .collect();

    TrashPageTemplate { photos }.try_into_response()
}

pub async fn restore_photo(
    auth_session: AuthSession,
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
) -> HttpResult<Response> {
    let user = auth_session.user.expect("User must be authenticated");

    let mut tx = state.pool.begin().await?;

    let mut photo = tx
        .get_photo(photo_id, &user.id)
        .await?
        .ok_or(HttpError::NotFound)?;

    photo.trashed_on = None;
    tx.update_photo(&photo).await?;
    tx.commit().await?;

    // Return empty response with HX-Trigger to refresh the page
    Ok(([("HX-Refresh", "true")]).into_response())
}

pub async fn permanent_delete(
    auth_session: AuthSession,
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
) -> HttpResult<Response> {
    let user = auth_session.user.expect("User must be authenticated");

    let mut tx = state.pool.begin().await?;

    let photo = tx
        .get_photo(photo_id, &user.id)
        .await?
        .ok_or(HttpError::NotFound)?;

    // Delete preview file
    let _ = fs::remove_file(state.storage.resolve_preview(photo.partial_preview_path())).await;

    // Delete photo file
    let photo_path = state.storage.resolve_photo(photo.partial_path());
    if photo_path.exists() {
        fs::remove_file(&photo_path).await?;
        info!("Removed file at {}", photo_path.display());
    } else {
        warn!("No such file exists at {}", photo_path.display());
    }

    tx.delete_photo(&photo).await?;
    tx.commit().await?;

    // Return empty response with HX-Trigger to refresh the page
    Ok(([("HX-Refresh", "true")]).into_response())
}
