use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{patch, post},
};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use crate::{
    http::{
        AppStateRef,
        auth::AuthenticatedUser,
        error::{HttpError, HttpResult, is_unique_violation},
    },
    model::user::User,
    repo::{FolderPermissionsRepo, FoldersRepo},
    utils::file_names::validate_folder_name,
};

#[derive(Serialize)]
pub(crate) struct FolderResponse {
    pub id: i64,
    pub owner_id: Option<String>,
    pub name: String,
}

pub fn router() -> Router<AppStateRef> {
    Router::new()
        .route("/", post(create_folder))
        .route("/{id}", patch(update_folder).delete(delete_folder))
}

#[derive(Deserialize)]
struct NewFolderRequest {
    name: String,
    is_public: bool,
}

async fn create_folder(
    State(state): State<AppStateRef>,
    AuthenticatedUser(user): AuthenticatedUser,
    Json(new_folder): Json<NewFolderRequest>,
) -> HttpResult<impl IntoResponse> {
    let folder = create_folder_for(state, &user, &new_folder.name, new_folder.is_public).await?;
    Ok(Json(folder))
}

/// Creates a folder (or returns the existing one with that name). Shared with the web UI's form.
pub(crate) async fn create_folder_for(
    state: AppStateRef,
    user: &User,
    name: &str,
    is_public: bool,
) -> HttpResult<FolderResponse> {
    let name =
        validate_folder_name(name).map_err(|message| HttpError::BadRequest(message.to_string()))?;
    let name = name.as_str();

    let owner_id = if is_public {
        None
    } else {
        Some(user.id.as_str())
    };

    let folder_id = state
        .write_pool
        .upsert_folder(owner_id, Some(name))
        .await?
        .ok_or(HttpError::BadRequest("Failed to create folder".to_string()))?;

    Ok(FolderResponse {
        id: folder_id,
        owner_id: owner_id.map(ToOwned::to_owned),
        name: name.to_owned(),
    })
}

fn folder_exists() -> HttpError {
    HttpError::Conflict("A folder with this name already exists".to_string())
}

fn map_unique_violation(error: sqlx::Error) -> HttpError {
    if is_unique_violation(&error) {
        folder_exists()
    } else {
        error.into()
    }
}

#[derive(Deserialize)]
struct UpdateFolderRequest {
    name: Option<String>,
    is_public: Option<bool>,
}

async fn update_folder(
    State(state): State<AppStateRef>,
    AuthenticatedUser(user): AuthenticatedUser,
    Path(folder_id): Path<i64>,
    Json(request): Json<UpdateFolderRequest>,
) -> HttpResult<impl IntoResponse> {
    let folder =
        update_folder_for(state, &user, folder_id, request.name, request.is_public).await?;
    Ok(Json(folder))
}

/// Renames a folder and/or changes its visibility. Shared with the web UI's form.
pub(crate) async fn update_folder_for(
    state: AppStateRef,
    user: &User,
    folder_id: i64,
    name: Option<String>,
    is_public: Option<bool>,
) -> HttpResult<FolderResponse> {
    let mut tx = state.write_pool.begin().await?;

    let folder = tx
        .as_mut()
        .get_accessible_folder(&user.id, folder_id)
        .await?
        .ok_or(HttpError::NotFound)?;

    if folder
        .owner_id
        .as_deref()
        .is_some_and(|owner| owner != user.id)
    {
        return Err(HttpError::Unauthorized);
    }

    // A blank name means "don't rename" (as before)
    let new_name = name
        .filter(|n| !n.trim().is_empty())
        .map(|n| validate_folder_name(&n))
        .transpose()
        .map_err(|message| HttpError::BadRequest(message.to_string()))?;
    let new_owner_id = is_public.map(|is_public| {
        if is_public {
            None
        } else {
            Some(user.id.clone())
        }
    });

    if new_name.is_none() && new_owner_id.is_none() {
        return Ok(FolderResponse {
            id: folder_id,
            owner_id: folder.owner_id.clone(),
            name: folder.name.clone(),
        });
    }

    let source_owner = folder.owner_id.as_deref();
    let target_owner = new_owner_id
        .as_ref()
        .map(|o| o.as_deref())
        .unwrap_or(source_owner);
    let target_name = new_name.as_deref().unwrap_or(&folder.name);

    let source_path = state
        .storage
        .resolve_folder(source_owner, Some(&folder.name));
    let target_path = state
        .storage
        .resolve_folder(target_owner, Some(target_name));

    if source_path != target_path && tokio::fs::try_exists(&target_path).await? {
        return Err(folder_exists());
    }

    if let Some(name) = &new_name {
        tx.rename_folder(folder_id, name)
            .await
            .map_err(map_unique_violation)?;
    }

    if let Some(owner) = &new_owner_id {
        tx.update_folder_owner(folder_id, owner.as_deref())
            .await
            .map_err(map_unique_violation)?;
        if owner.is_none() {
            // Public folders are visible to everyone, so per-user shares are meaningless
            tx.delete_grantee_shares(folder_id).await?;
        }
    }

    // Folders without photos may have no directory yet
    let move_directory = source_path != target_path && tokio::fs::try_exists(&source_path).await?;

    if move_directory {
        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::rename(&source_path, &target_path)
            .await
            .inspect_err(|e| warn!("Failed to rename folder: {e}"))?;
    }

    if let Err(e) = tx.commit().await {
        error!("Failed to commit folder update: {e}");
        if move_directory && let Err(e) = tokio::fs::rename(&target_path, &source_path).await {
            error!("Failed to undo folder rename: {e}");
        }
        return Err(e.into());
    }

    info!(
        "Updated folder {folder_id}: {:?} -> {target_name:?}, public: {}",
        folder.name,
        target_owner.is_none()
    );

    Ok(FolderResponse {
        id: folder_id,
        owner_id: target_owner.map(ToOwned::to_owned),
        name: target_name.to_owned(),
    })
}

/// Deletes a folder that has no photos at all (trashed ones included)
async fn delete_folder(
    State(state): State<AppStateRef>,
    AuthenticatedUser(user): AuthenticatedUser,
    Path(folder_id): Path<i64>,
) -> HttpResult<impl IntoResponse> {
    delete_folder_for(state, &user, folder_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Deletes a folder that has no photos at all. Shared with the web UI.
pub(crate) async fn delete_folder_for(
    state: AppStateRef,
    user: &User,
    folder_id: i64,
) -> HttpResult<()> {
    let mut tx = state.write_pool.begin().await?;

    let folder = tx
        .as_mut()
        .get_accessible_folder(&user.id, folder_id)
        .await?
        .ok_or(HttpError::NotFound)?;

    if folder
        .owner_id
        .as_deref()
        .is_some_and(|owner| owner != user.id)
    {
        return Err(HttpError::Unauthorized);
    }

    if tx.as_mut().count_folder_photos(folder_id).await? > 0 {
        return Err(HttpError::Conflict(
            "Move or delete the photos in this folder first".to_string(),
        ));
    }

    tx.as_mut().delete_folder(folder_id).await?;
    tx.commit().await?;

    // The directory may not exist (folders get one with their first photo). Anything left in it
    // isn't a known photo, so leave it alone.
    let path = state
        .storage
        .resolve_folder(folder.owner_id.as_deref(), Some(&folder.name));
    match tokio::fs::remove_dir(&path).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => warn!("Failed to remove the directory of deleted folder {folder_id}: {e}"),
    }

    info!(
        user = user.id,
        folder_id,
        name = folder.name,
        "Folder deleted"
    );

    Ok(())
}
