use crate::http::AppStateRef;
use crate::http::error::{HttpError, HttpResult};
use crate::http::utils::AuthSession;
use crate::model::photo::Photo;
use crate::model::user::PUBLIC_USER_FOLDER;
use crate::repo::{PhotosRepo, PhotosTransactionRepo};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use sqlx::Acquire;
use tracing::{error, info, warn};

pub fn router() -> Router<AppStateRef> {
    Router::new()
        .route("/folder", post(move_folder))
        .route("/photos", post(move_photos))
        .route("/{photo_id}", post(move_photo)) // TODO: Remove
}
#[derive(serde::Deserialize)]
struct RenameFolderQuery {
    source_is_public: bool,
    source_folder_name: String,
    target_make_public: bool,
    target_folder_name: Option<String>,
}

async fn move_folder(
    State(state): State<AppStateRef>,
    Query(query): Query<RenameFolderQuery>,
    auth: AuthSession,
) -> HttpResult<impl IntoResponse> {
    let user = auth.user.ok_or(HttpError::Unauthorized)?;

    let source_user_name = (!query.source_is_public).then_some(user.id.as_str());
    let target_user_name = (!query.target_make_public).then_some(user.id.as_str());

    let target_folder_name = query.target_folder_name.filter(|s| !s.is_empty());

    let photos_to_move = state
        .pool
        .get_photo_ids_in_folder(source_user_name, &query.source_folder_name)
        .await?;

    info!(
        "Moving folder \"{}/{}\" to \"{}/{}\" with {} items",
        source_user_name.unwrap_or(PUBLIC_USER_FOLDER),
        query.source_folder_name,
        target_user_name.unwrap_or(PUBLIC_USER_FOLDER),
        target_folder_name.as_deref().unwrap_or(""),
        photos_to_move.len(),
    );

    let moved_photos = move_photos_service(
        &photos_to_move,
        &user.id,
        target_user_name.map(ToOwned::to_owned),
        target_folder_name,
        state,
    )
    .await?;

    Ok(Json(moved_photos))
}

#[derive(serde::Deserialize)]
struct MovePhotoQuery {
    make_public: bool,
    target_folder_name: Option<String>,
}

async fn move_photo(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    Query(query): Query<MovePhotoQuery>,
    auth: AuthSession,
) -> HttpResult<impl IntoResponse> {
    let user = auth.user.ok_or(HttpError::Unauthorized)?;

    let target_user_name = (!query.make_public).then_some(user.id.clone());
    let target_folder_name = query.target_folder_name.filter(|s| !s.is_empty());

    let mut changed_photos = move_photos_service(
        &[photo_id],
        &user.id,
        target_user_name,
        target_folder_name,
        state,
    )
    .await?;

    let Some(changed_photo) = changed_photos.pop() else {
        warn!("Failed to move photo");
        return Ok(StatusCode::INTERNAL_SERVER_ERROR.into_response());
    };

    Ok(Json(changed_photo).into_response())
}

#[derive(serde::Deserialize)]
struct MovePhotosQuery {
    make_public: bool,
    target_folder_name: Option<String>,
}

async fn move_photos(
    State(state): State<AppStateRef>,
    Query(query): Query<MovePhotosQuery>,
    auth: AuthSession,
    Json(photos): Json<Vec<i64>>,
) -> HttpResult<impl IntoResponse> {
    let user = auth.user.ok_or(HttpError::Unauthorized)?;

    let target_user_name = (!query.make_public).then_some(user.id.clone());
    let target_folder_name = query.target_folder_name.filter(|s| !s.is_empty());

    let changed_photos = move_photos_service(
        &photos,
        &user.id,
        target_user_name,
        target_folder_name,
        state,
    )
    .await?;

    Ok(Json(changed_photos))
}

async fn move_photos_service(
    photo_ids: &[i64],
    user_id: &str,
    target_user_name: Option<String>,
    target_folder_name: Option<String>,
    state: AppStateRef,
) -> sqlx::Result<Vec<Photo>> {
    let mut moved_photos = Vec::with_capacity(photo_ids.len());

    // Acquire a connection from the pool
    let mut conn = state.pool.acquire().await?;

    for photo_id in photo_ids {
        // Create a separate transaction for each photo
        let mut tx = conn.begin().await?;

        let Some(mut photo) = tx.get_photo(*photo_id, user_id).await? else {
            continue;
        };
        let source_path = photo.partial_path();

        photo.user_id = target_user_name.clone();
        photo.folder = target_folder_name.clone();
        let destination_path = photo.partial_path();

        if source_path == destination_path {
            warn!(
                "Source and destination are the same: {destination_path}. Photo cannot be moved."
            );
            continue;
        }

        tx.update_photo(&photo).await?;

        if let Err(e) = state.storage.move_photo(&source_path, &destination_path) {
            error!("Failed to move the photo: {e}");
            continue;
        }

        if let Err(e) = tx.commit().await {
            // If the database operation failed for some reason, try to move the image back
            error!("Failed to commit transaction: {e}");
            if let Err(e) = state.storage.move_photo(&destination_path, &source_path) {
                error!("Failed to move the photo back: {e}");
            }
            continue;
        }

        info!("Moved photo from {source_path} to {destination_path}");
        moved_photos.push(photo);
    }

    Ok(moved_photos)
}
