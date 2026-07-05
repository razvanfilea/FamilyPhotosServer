use crate::http::AppStateRef;
use crate::http::auth::AuthenticatedUser;
use crate::http::error::{HttpError, HttpResult};
use crate::model::photo::Photo;
use crate::model::user::PUBLIC_USER_FOLDER;
use crate::repo::{FoldersRepo, PhotoAccess, PhotosRepo, PhotosTransactionRepo};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use sqlx::Acquire;
use tracing::{error, info, warn};

pub fn router() -> Router<AppStateRef> {
    Router::new()
        .route("/folder", post(move_folder))
        .route("/photos", post(move_photos))
}
#[derive(serde::Deserialize)]
struct RenameFolderQuery {
    source_folder_id: i64,
    target_make_public: bool,
    target_folder_name: Option<String>,
}

async fn move_folder(
    State(state): State<AppStateRef>,
    Query(query): Query<RenameFolderQuery>,
    AuthenticatedUser(user): AuthenticatedUser,
) -> HttpResult<impl IntoResponse> {
    let mut tx = state.write_pool.begin().await?;
    let Some(folder) = tx
        .get_accessible_folder(&user.id, query.source_folder_id)
        .await?
    else {
        return Err(HttpError::NotFound);
    };

    if folder.owner_id.as_deref().is_some_and(|owner| owner != user.id) {
        return Err(HttpError::NotFound);
    }

    let source_user_name = folder.owner_id.as_deref();
    let target_user_name = (!query.target_make_public).then_some(user.id.as_str());
    let target_folder_name = query.target_folder_name.filter(|s| !s.is_empty());

    info!(
        "Renaming folder \"{}/{}\" to \"{}/{}\"",
        source_user_name.unwrap_or(PUBLIC_USER_FOLDER),
        folder.name,
        target_user_name.unwrap_or(PUBLIC_USER_FOLDER),
        target_folder_name.as_deref().unwrap_or(""),
    );

    if let Some(new_name) = &target_folder_name {
        tx.rename_folder(folder.id, new_name).await?;

        if source_user_name != target_user_name {
            tx.update_folder_owner(folder.id, target_user_name).await?;
        }

        let moved_photos = tx.get_photos_in_folder(folder.id).await?;

        let source_folder = state
            .storage
            .resolve_folder(source_user_name, Some(&folder.name));
        let target_folder = state
            .storage
            .resolve_folder(target_user_name, target_folder_name.as_deref());

        tokio::fs::rename(&source_folder, &target_folder)
            .await
            .inspect_err(|e| warn!("Failed to rename folder: {e}"))?;

        if let Err(e) = tx.commit().await {
            // If the database operation failed for some reason, try to rename the folder back
            error!("Failed to commit transaction: {e}");

            if let Err(e) = tokio::fs::rename(target_folder, source_folder).await {
                error!("Failed to undo folder rename back: {e}");
                // I'm not sure there is anything else we can try to do
            }
            return Err(e.into());
        }

        info!("Folder renamed successfuly");

        Ok(Json(moved_photos))
    } else {
        let photos_to_move = tx
            .get_photo_ids_in_folder(source_user_name, folder.id)
            .await?;
        tx.commit().await?;

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
}

#[derive(serde::Deserialize)]
struct MovePhotosQuery {
    make_public: bool,
    target_folder_name: Option<String>,
}

async fn move_photos(
    State(state): State<AppStateRef>,
    Query(query): Query<MovePhotosQuery>,
    AuthenticatedUser(user): AuthenticatedUser,
    Json(photos): Json<Vec<i64>>,
) -> HttpResult<impl IntoResponse> {
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

    let mut conn = state.write_pool.acquire().await?;

    for photo_id in photo_ids {
        let mut tx = conn.begin().await?;

        let Some(pf) = tx
            .get_accessible_photo_with_folder(*photo_id, user_id, PhotoAccess::Delete)
            .await?
        else {
            continue;
        };
        let source_path = pf.partial_path();
        let mut photo = pf.photo;

        let target_folder_id = tx
            .upsert_folder(target_user_name.as_deref(), target_folder_name.as_deref())
            .await?;

        photo.user_id = target_user_name.clone();
        photo.folder_id = target_folder_id;
        let destination_path = photo.partial_path(target_folder_name.as_deref());

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
                error!("Failed to undo the photo move: {e}");
            }
            continue;
        }

        info!("Moved photo from {source_path} to {destination_path}");
        moved_photos.push(photo);
    }

    Ok(moved_photos)
}
