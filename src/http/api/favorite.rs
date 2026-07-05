use crate::http::AppStateRef;
use crate::http::auth::AuthenticatedUser;
use crate::http::error::{HttpError, HttpResult};
use crate::repo::{FavoritesRepo, PhotoAccess, PhotosRepo};
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
    AuthenticatedUser(user): AuthenticatedUser,
) -> HttpResult<impl IntoResponse> {
    Ok(Json(
        state
            .read_pool
            .get_favorite_photos(user.id.as_str())
            .await?,
    ))
}

async fn add_favorite(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    AuthenticatedUser(user): AuthenticatedUser,
) -> HttpResult<impl IntoResponse> {
    let mut tx = state.write_pool.begin().await?;

    tx.get_accessible_photo(photo_id, &user.id, PhotoAccess::Read)
        .await?
        .ok_or(HttpError::NotFound)?;

    tx.insert_favorite(photo_id, &user.id).await?;

    tx.commit().await?;

    Ok(())
}

async fn delete_favorite(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    AuthenticatedUser(user): AuthenticatedUser,
) -> HttpResult<impl IntoResponse> {
    let mut tx = state.write_pool.begin().await?;

    tx.get_accessible_photo(photo_id, &user.id, PhotoAccess::Read)
        .await?
        .ok_or(HttpError::NotFound)?;

    tx.delete_favorite(photo_id, &user.id).await?;

    tx.commit().await?;

    Ok(())
}
