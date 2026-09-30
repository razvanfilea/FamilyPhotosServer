use crate::http::AppStateRef;
use crate::http::auth::AuthenticatedUser;
use crate::http::error::{HttpError, HttpResult};
use crate::model::photo::Photo;
use crate::model::user::SimpleUser;
use crate::repo::{FavoritesRepo, PhotoAccess, PhotosRepo};
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
    let mut tx = state.read_pool.begin().await?;

    let pf = tx
        .get_accessible_photo_with_folder(photo_id, user_id, PhotoAccess::Read)
        .await?
        .ok_or(HttpError::NotFound)?;
    let can_delete = tx
        .get_accessible_photo(photo_id, user_id, PhotoAccess::Delete)
        .await?
        .is_some();
    let is_favorite = tx.check_favorite(photo_id, user_id).await?;
    drop(tx);

    let owner = match pf.photo.user_id.as_deref() {
        Some(owner_id) => state
            .users_repo
            .get_user(owner_id)
            .await
            .map(SimpleUser::from),
        None => None,
    };

    let path = state.storage.resolve_photo(pf.partial_path());
    let name = pf.photo.name.clone();
    let details = task::spawn_blocking(move || photo_details(&path, &name))
        .await
        .map_err(|e| HttpError::AnyError(Box::new(e)))?;

    let folder = match (pf.photo.folder_id, pf.folder_name) {
        (Some(id), Some(name)) => Some(FolderRef { id, name }),
        _ => None,
    };

    Ok(PhotoInfo {
        photo: pf.photo,
        folder,
        owner,
        is_favorite,
        can_delete,
        can_move: can_delete,
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
