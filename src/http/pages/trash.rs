use crate::http::AppStateRef;
use crate::http::error::{HttpError, HttpResult};
use crate::http::pages::gallery::{PhotoView, extract_grouped_folders, fetch_photos_and_favorites};
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::utils::AuthSession;
use crate::repo::{PhotosRepo, PhotosTransactionRepo};
use askama::Template;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use tokio::fs;
use tracing::{info, warn};

#[derive(Template)]
#[template(path = "trash/trash_page.html")]
struct TrashPageTemplate {
    photos: Vec<PhotoView>,
    personal_folders: Vec<String>,
    family_folders: Vec<String>,
}

pub async fn trash_page(
    auth_session: AuthSession,
    State(state): State<AppStateRef>,
) -> HttpResult<Response> {
    let user = auth_session.user.expect("User must be authenticated");

    let data = fetch_photos_and_favorites(&state, &user.id).await?;
    let grouped_folders = extract_grouped_folders(&data.photos, &user.id);

    let photos: Vec<PhotoView> = data
        .photos
        .into_iter()
        .filter(|p| p.trashed_on.is_some())
        .map(|p| PhotoView::from_photo(p, &data.favorite_ids))
        .collect();

    TrashPageTemplate {
        photos,
        personal_folders: grouped_folders.personal,
        family_folders: grouped_folders.family,
    }
    .try_into_response()
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
