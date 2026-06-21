use crate::http::AppStateRef;
use crate::http::error::{HttpError, HttpResult};
use crate::http::utils::AuthSession;
use crate::model::photo::Photo;
use crate::repo::{PhotosRepo, PhotosTransactionRepo};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use time::OffsetDateTime;

pub fn router() -> Router<AppStateRef> {
    Router::new()
        .route("/", post(trash_photos))
        .route("/restore", post(restore_photos))
}

async fn trash_photos(
    State(state): State<AppStateRef>,
    auth_session: AuthSession,
    Json(photo_ids): Json<Vec<i64>>,
) -> HttpResult<impl IntoResponse> {
    let user = auth_session.user.ok_or(HttpError::Unauthorized)?;
    let mut tx = state.write_pool.begin().await?;
    let now = OffsetDateTime::now_utc();

    let mut photos: Vec<Photo> = Vec::with_capacity(photo_ids.len());

    for photo_id in photo_ids {
        let mut photo = tx
            .get_accessible_photo(photo_id, &user.id)
            .await?
            .ok_or(HttpError::NotFound)?;

        photo.trashed_on = Some(now);
        tx.update_photo(&photo).await?;
        photos.push(photo);
    }

    tx.commit().await?;

    Ok(Json(photos))
}

async fn restore_photos(
    State(state): State<AppStateRef>,
    auth_session: AuthSession,
    Json(photo_ids): Json<Vec<i64>>,
) -> HttpResult<impl IntoResponse> {
    let user = auth_session.user.ok_or(HttpError::Unauthorized)?;
    let mut tx = state.write_pool.begin().await?;

    let mut photos: Vec<Photo> = Vec::with_capacity(photo_ids.len());

    for photo_id in photo_ids {
        let mut photo = tx
            .get_accessible_photo(photo_id, &user.id)
            .await?
            .ok_or(HttpError::NotFound)?;

        photo.trashed_on = None;
        tx.update_photo(&photo).await?;
        photos.push(photo);
    }

    tx.commit().await?;

    Ok(Json(photos))
}
