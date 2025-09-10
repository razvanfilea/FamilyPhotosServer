use crate::http::AppStateRef;
use crate::http::error::{HttpError, HttpResult};
use crate::http::utils::AuthSession;
use crate::repo::{FavoritesRepo, PhotosRepo};
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
) -> HttpResult<impl IntoResponse> {
    let user = auth_session.user.ok_or(HttpError::Unauthorized)?;

    Ok(Json(
        state.pool.get_favorite_photos(user.id.as_str()).await?,
    ))
}

async fn add_favorite(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    auth_session: AuthSession,
) -> HttpResult<impl IntoResponse> {
    let user = auth_session.user.ok_or(HttpError::Unauthorized)?;
    let mut tx = state.pool.begin().await?;

    tx.get_photo(photo_id, &user.id)
        .await?
        .ok_or(HttpError::NotFound)?;

    tx.insert_favorite(photo_id, &user.id).await?;

    tx.commit().await?;

    Ok(())
}

async fn delete_favorite(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    auth_session: AuthSession,
) -> HttpResult<impl IntoResponse> {
    let user = auth_session.user.ok_or(HttpError::Unauthorized)?;
    let mut tx = state.pool.begin().await?;

    tx.get_photo(photo_id, &user.id)
        .await?
        .ok_or(HttpError::NotFound)?;

    tx.delete_favorite(photo_id, &user.id).await?;

    tx.commit().await?;

    Ok(())
}
