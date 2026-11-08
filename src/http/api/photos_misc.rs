use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
};
use time::OffsetDateTime;
use tokio::{fs, task};
use tracing::{info, warn};

use crate::http::{AppStateRef, auth::AuthenticatedUser};
use crate::model::photo::Photo;
use crate::repo::{PhotoAccess, PhotosHashRepo, PhotosRepo, PhotosTransactionRepo};
use crate::{
    http::error::{HttpError, HttpResult},
    previews,
    utils::exif::read_exif,
};
use time::serde::timestamp;

pub fn router() -> Router<AppStateRef> {
    Router::new()
        .route("/timestamp/{photo_id}", post(update_timestamp))
        .route("/photos/date", post(update_photos_date))
        .route("/duplicates", get(get_duplicates))
        .route("/delete/{photo_id}", delete(delete_photo))
        .route("/exif/{photo_id}", get(get_photo_exif))
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

#[derive(serde::Deserialize)]
struct UpdateDateBody {
    photo_ids: Vec<i64>,
    created_at: i64,
}

async fn update_photos_date(
    State(state): State<AppStateRef>,
    AuthenticatedUser(user): AuthenticatedUser,
    body: Result<Json<UpdateDateBody>, JsonRejection>,
) -> HttpResult<Json<Vec<Photo>>> {
    const MAX_DATE_UPDATE_PHOTOS: usize = 1000;
    /// The first photograph dates from 1826: anything older is a typo
    const MIN_PHOTO_DATE: i64 = -4_544_208_000; // 1826-01-01T00:00:00Z
    /// Slack for clocks that run ahead
    const MAX_FUTURE_SECS: i64 = 24 * 60 * 60;

    let Json(body) = body.map_err(|e| HttpError::BadRequest(e.body_text()))?;

    let now = OffsetDateTime::now_utc().unix_timestamp();
    if !(MIN_PHOTO_DATE..=now + MAX_FUTURE_SECS).contains(&body.created_at) {
        return Err(HttpError::BadRequest("The date is out of range".into()));
    }
    let created_at = OffsetDateTime::from_unix_timestamp(body.created_at)
        .map_err(|_| HttpError::BadRequest("Invalid date".into()))?;

    if body.photo_ids.len() > MAX_DATE_UPDATE_PHOTOS {
        return Err(HttpError::BadRequest("Too many photos".into()));
    }
    let mut photo_ids = body.photo_ids;
    photo_ids.sort_unstable();
    photo_ids.dedup();

    let mut editable = Vec::with_capacity(photo_ids.len());
    for id in photo_ids {
        let photo = state
            .read_pool
            .get_accessible_photo(id, &user.id, PhotoAccess::Own)
            .await?;
        if photo.is_some_and(|p| p.trashed_on.is_none()) {
            editable.push(id);
        }
    }
    if editable.is_empty() {
        return Ok(Json(Vec::new()));
    }

    let mut tx = state.write_pool.begin().await?;
    let mut updated = Vec::with_capacity(editable.len());
    for id in editable {
        let Some(mut photo) = tx
            .get_accessible_photo(id, &user.id, PhotoAccess::Own)
            .await?
            .filter(|p| p.trashed_on.is_none())
        else {
            continue;
        };
        photo.created_at = created_at;
        tx.update_photo(&photo).await?;
        updated.push(photo);
    }
    tx.commit().await?;

    Ok(Json(updated))
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

    previews::remove_renditions(&state.storage, pf.photo.id).await;

    let photo_path = state.storage.resolve_photo(pf.partial_path());
    if fs::try_exists(&photo_path).await? {
        fs::remove_file(&photo_path).await?;
        info!("Removed file at {}", photo_path.display());
    } else {
        warn!("No such file exists at {}", photo_path.display());
    }

    tx.delete_photo(&pf.photo).await?;

    tx.commit().await?;

    Ok(())
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
