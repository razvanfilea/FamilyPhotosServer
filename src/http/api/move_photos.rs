use crate::http::AppStateRef;
use crate::http::auth::AuthenticatedUser;
use crate::http::error::{HttpError, HttpResult};
use crate::model::photo::Photo;
use crate::repo::{FoldersRepo, PhotoAccess, PhotosRepo, PhotosTransactionRepo};
use axum::extract::{Query, State};
use axum::routing::post;
use axum::{Json, Router};
use sqlx::Acquire;
use tracing::{error, info, warn};

pub fn router() -> Router<AppStateRef> {
    Router::new().route("/", post(move_photos))
}

#[derive(serde::Deserialize)]
struct MovePhotosQuery {
    #[serde(default)]
    make_public: bool,
    target_folder_id: Option<i64>,
}

async fn move_photos(
    State(state): State<AppStateRef>,
    AuthenticatedUser(user): AuthenticatedUser,
    Query(query): Query<MovePhotosQuery>,
    Json(photo_ids): Json<Vec<i64>>,
) -> HttpResult<Json<Vec<Photo>>> {
    let target_folder = match query.target_folder_id {
        Some(id) => {
            let folder = state
                .read_pool
                .get_accessible_folder(&user.id, id)
                .await?
                .ok_or(HttpError::NotFound)?;
            if !folder.can_upload {
                return Err(HttpError::NotFound);
            }
            Some(folder)
        }
        None => None,
    };

    let owner_id = match &target_folder {
        Some(folder) => folder.owner_id.clone(),
        None if query.make_public => None,
        None => Some(user.id.clone()),
    };

    let target_folder_name = target_folder.as_ref().map(|f| f.name.as_str());
    let target_folder_id = target_folder.as_ref().map(|f| f.id);

    let mut moved_photos = Vec::with_capacity(photo_ids.len());
    let mut conn = state.write_pool.acquire().await?;

    for photo_id in photo_ids {
        let mut tx = conn.begin().await?;

        let Some(pf) = tx
            .get_accessible_photo_with_folder(photo_id, &user.id, PhotoAccess::Delete)
            .await?
        else {
            continue;
        };
        let source_path = pf.partial_path();
        let mut photo = pf.photo;

        photo.user_id = owner_id.clone();
        photo.folder_id = target_folder_id;
        let destination_path = photo.partial_path(target_folder_name);

        if source_path == destination_path {
            warn!("Source and destination are the same: {destination_path}. Photo cannot be moved.");
            continue;
        }

        tx.update_photo(&photo).await?;

        if let Err(e) = state.storage.move_photo(&source_path, &destination_path) {
            error!("Failed to move the photo: {e}");
            continue;
        }

        if let Err(e) = tx.commit().await {
            error!("Failed to commit transaction: {e}");
            if let Err(e) = state.storage.move_photo(&destination_path, &source_path) {
                error!("Failed to undo the photo move: {e}");
            }
            continue;
        }

        info!("Moved photo from {source_path} to {destination_path}");
        moved_photos.push(photo);
    }

    Ok(Json(moved_photos))
}
