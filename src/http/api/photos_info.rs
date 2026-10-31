use crate::http::AppStateRef;
use crate::http::auth::AuthenticatedUser;
use crate::http::error::{HttpError, HttpResult};
use crate::model::photo::Photo;
use crate::model::user::SimpleUser;
use crate::repo::PhotosRepo;
use crate::utils::exif::{PhotoDetails, photo_details};
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use tokio::task;

pub fn router() -> Router<AppStateRef> {
    Router::new().route("/photos/{photo_id}/info", get(photo_info))
}

#[derive(Debug, Serialize)]
pub struct FolderRef {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct PhotoInfo {
    pub photo: Photo,
    pub folder: Option<FolderRef>,
    /// `null` for public photos
    pub owner: Option<SimpleUser>,
    pub is_favorite: bool,
    pub can_delete: bool,
    pub can_move: bool,
    pub details: PhotoDetails,
}

/// Loads everything the info panel shows. Read access; 404 otherwise.
pub async fn load_photo_info(
    state: AppStateRef,
    user_id: &str,
    photo_id: i64,
) -> HttpResult<PhotoInfo> {
    let info = state
        .read_pool
        .get_photo_info(photo_id, user_id)
        .await?
        .ok_or(HttpError::NotFound)?;
    let photo = info.photo;
    let owner = photo
        .user_id
        .clone()
        .zip(info.owner_name)
        .map(|(user_id, display_name)| SimpleUser {
            user_id,
            display_name,
        });

    let path = state
        .storage
        .resolve_photo(photo.partial_path(info.folder_name.as_deref()));
    let name = photo.name.clone();
    let details = task::spawn_blocking(move || photo_details(&path, &name))
        .await
        .map_err(|e| HttpError::AnyError(Box::new(e)))?;

    let folder = photo
        .folder_id
        .zip(info.folder_name)
        .map(|(id, name)| FolderRef { id, name });

    Ok(PhotoInfo {
        photo,
        folder,
        owner,
        is_favorite: info.is_favorite,
        can_delete: info.can_delete,
        can_move: info.can_delete,
        details,
    })
}

async fn photo_info(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    AuthenticatedUser(user): AuthenticatedUser,
) -> HttpResult<Json<PhotoInfo>> {
    Ok(Json(load_photo_info(state, &user.id, photo_id).await?))
}
