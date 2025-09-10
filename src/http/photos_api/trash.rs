use crate::http::AppStateRef;
use crate::http::error::{HttpError, HttpResult};
use crate::http::utils::AuthSession;
use crate::repo::{PhotosRepo, PhotosTransactionRepo};
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::{delete, post};
use axum::{Json, Router};
use time::OffsetDateTime;

pub fn router() -> Router<AppStateRef> {
    Router::new()
        .route("/{photo_id}", post(trash_photo))
        .route("/{photo_id}", delete(restore_photo))
}

async fn trash_photo(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    auth_session: AuthSession,
) -> HttpResult<impl IntoResponse> {
    let user = auth_session.user.ok_or(HttpError::Unauthorized)?;
    let mut tx = state.pool.begin().await?;

    let mut photo = tx
        .get_photo(photo_id, &user.id)
        .await?
        .ok_or(HttpError::NotFound)?;

    photo.trashed_on = Some(OffsetDateTime::now_utc());

    tx.update_photo(&photo).await?;

    tx.commit().await?;

    Ok(Json(photo))
}

async fn restore_photo(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    auth_session: AuthSession,
) -> HttpResult<impl IntoResponse> {
    let user = auth_session.user.ok_or(HttpError::Unauthorized)?;
    let mut tx = state.pool.begin().await?;

    let mut photo = tx
        .get_photo(photo_id, &user.id)
        .await?
        .ok_or(HttpError::NotFound)?;

    photo.trashed_on = None;

    tx.update_photo(&photo).await?;

    tx.commit().await?;

    Ok(Json(photo))
}
