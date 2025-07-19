use crate::http::AppStateRef;
use crate::http::photos_api::check_has_access;
use crate::http::utils::{AuthSession, AxumResult};
use crate::utils::internal_error;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};

pub fn router() -> Router<AppStateRef> {
    Router::new()
        .route("/", get(get_favorites))
        .route("/{photo_id}", post(add_favorite))
        .route("/{photo_id}", delete(delete_favorite))
}
async fn get_favorites(
    State(state): State<AppStateRef>,
    auth_session: AuthSession,
) -> AxumResult<impl IntoResponse> {
    let user = auth_session.user.unwrap();

    Ok(Json(
        state
            .favorites_repo
            .get_favorite_photos(user.id.as_str())
            .await
            .map_err(internal_error)?,
    ))
}

async fn add_favorite(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    auth_session: AuthSession,
) -> AxumResult<impl IntoResponse> {
    let photo = state
        .photos_repo
        .get_photo(photo_id)
        .await
        .map_err(internal_error)?;
    let user = check_has_access(auth_session.user, &photo)?;

    state
        .favorites_repo
        .insert_favorite(photo_id, user.id)
        .await
        .map_err(internal_error)
}

async fn delete_favorite(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    auth_session: AuthSession,
) -> AxumResult<impl IntoResponse> {
    let photo = state
        .photos_repo
        .get_photo(photo_id)
        .await
        .map_err(internal_error)?;
    let user = check_has_access(auth_session.user, &photo)?;

    state
        .favorites_repo
        .delete_favorite(photo_id, user.id)
        .await
        .map_err(internal_error)
}
