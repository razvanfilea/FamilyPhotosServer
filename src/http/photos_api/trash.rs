use crate::http::AppStateRef;
use crate::http::utils::{AuthSession, AxumResult};
use crate::repo::{PhotosRepo, PhotosTransactionRepo};
use crate::utils::internal_error;
use axum::extract::{Path, State};
use axum::http::StatusCode;
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
) -> AxumResult<impl IntoResponse> {
    let user = auth_session.user.ok_or(StatusCode::UNAUTHORIZED)?;
    let mut tx = state.pool.begin().await.map_err(internal_error)?;

    let mut photo = tx
        .get_photo(photo_id, &user.id)
        .await
        .map_err(internal_error)?
        .ok_or(StatusCode::NOT_FOUND)?;

    photo.trashed_on = Some(OffsetDateTime::now_utc());

    tx.update_photo(&photo).await.map_err(internal_error)?;

    tx.commit().await.map_err(internal_error)?;

    Ok(Json(photo))
}

async fn restore_photo(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    auth_session: AuthSession,
) -> AxumResult<impl IntoResponse> {
    let user = auth_session.user.ok_or(StatusCode::UNAUTHORIZED)?;
    let mut tx = state.pool.begin().await.map_err(internal_error)?;

    let mut photo = tx
        .get_photo(photo_id, &user.id)
        .await
        .map_err(internal_error)?
        .ok_or(StatusCode::NOT_FOUND)?;

    photo.trashed_on = None;

    tx.update_photo(&photo).await.map_err(internal_error)?;

    tx.commit().await.map_err(internal_error)?;

    Ok(Json(photo))
}
