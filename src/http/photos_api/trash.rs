use crate::http::AppStateRef;
use crate::http::photos_api::check_has_access;
use crate::http::utils::{AuthSession, AxumResult};
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
    let mut photo = state
        .photos_repo
        .get_photo(photo_id)
        .await
        .map_err(internal_error)?
        .ok_or(StatusCode::NOT_FOUND)?;
    check_has_access(auth_session.user, &photo)?;

    photo.trashed_on = Some(OffsetDateTime::now_utc());

    state
        .photos_repo
        .update_photo(&photo)
        .await
        .map_err(internal_error)?;

    Ok(Json(photo))
}

async fn restore_photo(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    auth_session: AuthSession,
) -> AxumResult<impl IntoResponse> {
    let mut photo = state
        .photos_repo
        .get_photo(photo_id)
        .await
        .map_err(internal_error)?
        .ok_or(StatusCode::NOT_FOUND)?;
    check_has_access(auth_session.user, &photo)?;

    photo.trashed_on = None;

    state
        .photos_repo
        .update_photo(&photo)
        .await
        .map_err(internal_error)?;

    Ok(Json(photo))
}
