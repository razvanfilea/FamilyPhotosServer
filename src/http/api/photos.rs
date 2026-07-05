use axum::{
    Json, Router,
    extract::{Path, Query, State},
    response::IntoResponse,
    routing::{delete, get, post},
};
use time::OffsetDateTime;
use tokio::fs;
use tracing::{info, warn};

use crate::http::error::{HttpError, HttpResult};
use crate::http::{AppStateRef, auth::AuthenticatedUser};
use crate::repo::{PhotoAccess, PhotosHashRepo, PhotosRepo, PhotosTransactionRepo};
use time::serde::timestamp;

pub fn router() -> Router<AppStateRef> {
    Router::new()
        .route("/timestamp/{photo_id}", post(update_timestamp))
        .route("/duplicates", get(get_duplicates))
        .route("/delete/{photo_id}", delete(delete_photo))
}

#[derive(serde::Deserialize)]
struct UpdateTimeQuery {
    #[serde(with = "timestamp")]
    time_created: OffsetDateTime,
}

async fn update_timestamp(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    Query(query): Query<UpdateTimeQuery>,
    AuthenticatedUser(user): AuthenticatedUser,
) -> HttpResult<impl IntoResponse> {
    let mut tx = state.write_pool.begin().await?;

    let mut photo = tx
        .get_accessible_photo(photo_id, &user.id, PhotoAccess::Own)
        .await?
        .ok_or(HttpError::NotFound)?;

    photo.created_at = query.time_created;

    tx.update_photo(&photo).await?;

    tx.commit().await?;

    Ok(Json(photo))
}

async fn get_duplicates(
    State(state): State<AppStateRef>,
    AuthenticatedUser(user): AuthenticatedUser,
) -> HttpResult<impl IntoResponse> {
    let photos = state
        .read_pool
        .get_duplicates_for_user(user.id.as_str())
        .await?;

    Ok(Json(photos))
}

async fn delete_photo(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    AuthenticatedUser(user): AuthenticatedUser,
) -> HttpResult<impl IntoResponse> {
    let mut tx = state.write_pool.begin().await?;

    let pf = tx
        .get_accessible_photo_with_folder(photo_id, &user.id, PhotoAccess::Own)
        .await?
        .ok_or(HttpError::NotFound)?;

    if pf.photo.trashed_on.is_none() {
        return Err(HttpError::BadRequest(
            "Only a trashed photo can be permanently deleted".to_string(),
        ));
    }

    let _ = fs::remove_file(state.storage.resolve_preview(pf.partial_preview_path())).await;

    let photo_path = state.storage.resolve_photo(pf.partial_path());
    if photo_path.exists() {
        fs::remove_file(&photo_path).await?;
        info!("Removed file at {}", photo_path.display());
    } else {
        warn!("No such file exists at {}", photo_path.display());
    }

    tx.delete_photo(&pf.photo).await?;

    tx.commit().await?;

    Ok(())
}
