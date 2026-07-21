use axum::{
    Json, Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::{patch, post},
};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use crate::{
    http::{
        AppStateRef,
        auth::AuthenticatedUser,
        error::{HttpError, HttpResult},
    },
    repo::FoldersRepo,
};

#[derive(Serialize)]
struct FolderResponse {
    id: i64,
    owner_id: Option<String>,
    name: String,
}

pub fn router() -> Router<AppStateRef> {
    Router::new()
        .route("/", post(create_folder))
        .route("/{id}", patch(update_folder))
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
    let name = new_folder.name.trim();
    if name.is_empty() {
        return Err(HttpError::BadRequest(
            "Folder name cannot be empty".to_string(),
        ));
    }

    let owner_id = if new_folder.is_public {
        None
    } else {
        Some(user.id.as_str())
    };

    let folder_id = state
        .write_pool
        .upsert_folder(owner_id, Some(name))
        .await?
        .ok_or(HttpError::BadRequest("Failed to create folder".to_string()))?;

    Ok(Json(FolderResponse {
        id: folder_id,
        owner_id: owner_id.map(ToOwned::to_owned),
        name: name.to_owned(),
    }))
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

    let new_name = request
        .name
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty());
    let new_owner_id = request.is_public.map(|is_public| {
        if is_public {
            None
        } else {
            Some(user.id.clone())
        }
    });

    if new_name.is_none() && new_owner_id.is_none() {
        return Ok(Json(FolderResponse {
            id: folder_id,
            owner_id: folder.owner_id.clone(),
            name: folder.name.clone(),
        }));
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

    if let Some(name) = &new_name {
        tx.rename_folder(folder_id, name).await?;
    }

    if let Some(owner) = &new_owner_id {
        tx.update_folder_owner(folder_id, owner.as_deref()).await?;
    }

    if source_path != target_path {
        if let Some(parent) = target_path.parent()
            && !parent.exists()
        {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::rename(&source_path, &target_path)
            .await
            .inspect_err(|e| warn!("Failed to rename folder: {e}"))?;
    }

    if let Err(e) = tx.commit().await {
        error!("Failed to commit folder update: {e}");
        if source_path != target_path
            && let Err(e) = tokio::fs::rename(&target_path, &source_path).await
        {
            error!("Failed to undo folder rename: {e}");
        }
        return Err(e.into());
    }

    info!(
        "Updated folder {folder_id}: {:?} -> {target_name:?}, public: {}",
        folder.name,
        target_owner.is_none()
    );

    Ok(Json(FolderResponse {
        id: folder_id,
        owner_id: target_owner.map(ToOwned::to_owned),
        name: target_name.to_owned(),
    }))
}
