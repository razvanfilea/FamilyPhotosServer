use axum::{
    Json, Router,
    extract::{Multipart, Path, Query, State},
    response::IntoResponse,
    routing::{delete, get, post},
};
use axum_extra::TypedHeader;
use axum_extra::headers::Range;
use sqlx::__rt::timeout;
use std::time::Duration;
use time::OffsetDateTime;
use time::serde::timestamp;
use tokio::{fs, task};
use tracing::{error, info, warn};

use crate::http::AppStateRef;
use crate::http::error::{HttpError, HttpResult};
use crate::http::extractors::SharedPermission;
use crate::http::utils::{file_to_response, write_field_to_file};
use crate::model::folder_permission::ShareInfo;
use crate::model::photo::Photo;
use crate::previews;
use crate::repo::{PhotosHashRepo, PhotosRepo, PhotosTransactionRepo};

pub fn router(app_state: AppStateRef) -> Router {
    Router::new()
        .route("/{token}/info", get(share_info))
        .route("/{token}/photos", get(list_photos))
        .route("/{token}/download/{photo_id}", get(download_photo))
        .route("/{token}/preview/{photo_id}", get(preview_photo))
        .route("/{token}/upload", post(upload_photo))
        .route("/{token}/delete/{photo_id}", delete(delete_photo))
        .with_state(app_state)
}

async fn share_info(permission: SharedPermission) -> HttpResult<impl IntoResponse> {
    let info = ShareInfo::from(&permission.0);
    Ok(Json(info))
}

async fn list_photos(
    State(state): State<AppStateRef>,
    permission: SharedPermission,
) -> HttpResult<impl IntoResponse> {
    let perm = permission.0;
    let photos = state
        .pool
        .get_photos_in_folder(Some(&perm.owner_id), &perm.folder_name)
        .await?;
    Ok(Json(photos))
}

async fn download_photo(
    State(state): State<AppStateRef>,
    permission: SharedPermission,
    Path((_token, photo_id)): Path<(String, i64)>,
    range: Option<TypedHeader<Range>>,
) -> HttpResult<impl IntoResponse> {
    let perm = permission.0;
    let photo = state
        .pool
        .get_photo_in_shared_folder(photo_id, &perm.owner_id, &perm.folder_name)
        .await?
        .ok_or(HttpError::NotFound)?;

    let photo_path = state.storage.resolve_photo(photo.partial_path());
    file_to_response(&photo_path, range).await
}

async fn preview_photo(
    State(state): State<AppStateRef>,
    permission: SharedPermission,
    Path((_token, photo_id)): Path<(String, i64)>,
    range: Option<TypedHeader<Range>>,
) -> HttpResult<impl IntoResponse> {
    let perm = permission.0;
    let storage = &state.storage;

    let photo = state
        .pool
        .get_photo_in_shared_folder(photo_id, &perm.owner_id, &perm.folder_name)
        .await?
        .ok_or(HttpError::NotFound)?;

    let photo_path = storage.resolve_photo(photo.partial_path());
    let preview_path = storage.resolve_preview(photo.partial_preview_path());

    let preview_generation_mutex =
        timeout(Duration::from_secs(3), state.preview_generation.lock()).await;
    if preview_generation_mutex.is_err() {
        return file_to_response(&photo_path, range).await;
    }

    let needs_generation = match tokio::fs::metadata(&preview_path).await {
        Ok(m) => m.len() < previews::MIN_PREVIEW_SIZE,
        Err(_) => true,
    };

    let preview_generated = if needs_generation {
        let photo_path_clone = photo_path.clone();
        let preview_path_clone = preview_path.clone();

        task::spawn_blocking(move || {
            previews::generate_preview(photo_path_clone, preview_path_clone)
        })
        .await
        .map_err(|e| HttpError::AnyError(Box::new(e)))?
    } else {
        Ok(())
    };

    let path = match preview_generated {
        Ok(_) => preview_path,
        Err(e) => {
            error!(
                "Preview generation failed for: {}\nCause: {e}",
                photo_path.display()
            );
            photo_path
        }
    };

    file_to_response(&path, range).await
}

#[derive(Debug, serde::Deserialize)]
struct UploadDataQuery {
    #[serde(with = "timestamp")]
    time_created: OffsetDateTime,
}

async fn upload_photo(
    State(state): State<AppStateRef>,
    permission: SharedPermission,
    Query(query): Query<UploadDataQuery>,
    mut payload: Multipart,
) -> HttpResult<impl IntoResponse> {
    let perm = permission.0;

    if !perm.can_upload {
        return Err(HttpError::Unauthorized);
    }

    let field = payload
        .next_field()
        .await
        .map_err(|e| HttpError::AnyError(Box::new(e)))?
        .ok_or_else(|| HttpError::BadRequest("Multipart is empty".to_string()))?;

    let file_name = field
        .file_name()
        .or(field.name())
        .ok_or_else(|| HttpError::BadRequest("Multipart has no name".to_string()))?
        .to_owned();

    let written_file = write_field_to_file(field).await?;

    let mut tx = state.pool.begin().await?;
    let existing_photo = tx
        .get_photo_with_hash(&written_file.hash, Some(&perm.owner_id))
        .await?;

    if let Some(photo) = existing_photo {
        info!(
            "Photo with same hash already exists with path: {}",
            photo.partial_path()
        );
        return Ok(Json(photo));
    }

    let mut photo = Photo {
        id: 0,
        user_id: Some(perm.owner_id.clone()),
        name: file_name,
        created_at: query.time_created,
        file_size: written_file.size as i64,
        folder: Some(perm.folder_name.clone()),
        thumb_hash: None,
        trashed_on: None,
    };

    let mut photo_path = state.storage.resolve_photo(photo.partial_path());
    if let Some(parent) = photo_path.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent).await?;
    }

    if photo_path.exists() {
        photo.name = format!(
            "{}.{}",
            uuid::Uuid::new_v4(),
            photo_path
                .extension()
                .map(|str| str.to_string_lossy().to_string())
                .unwrap_or_default()
        );
        photo_path = state.storage.resolve_photo(photo.partial_path());
    }

    info!("Uploading file to {} via shared link", photo_path.display());

    let photo = tx.insert_photo(&photo).await?;

    written_file.persist_to(&photo_path).await?;

    tx.commit().await.inspect_err(|_| {
        if let Err(e) = std::fs::remove_file(&photo_path) {
            error!("Failed to remove uploaded file: {e}");
        }
    })?;

    Ok(Json(photo))
}

async fn delete_photo(
    State(state): State<AppStateRef>,
    permission: SharedPermission,
    Path((_token, photo_id)): Path<(String, i64)>,
) -> HttpResult<impl IntoResponse> {
    let perm = permission.0;

    if !perm.can_delete {
        return Err(HttpError::Unauthorized);
    }

    let mut tx = state.pool.begin().await?;

    let photo = tx
        .get_photo_in_shared_folder(photo_id, &perm.owner_id, &perm.folder_name)
        .await?
        .ok_or(HttpError::NotFound)?;

    let _ = fs::remove_file(state.storage.resolve_preview(photo.partial_preview_path())).await;

    let photo_path = state.storage.resolve_photo(photo.partial_path());
    if photo_path.exists() {
        fs::remove_file(&photo_path).await?;
        info!("Removed file at {} via shared link", photo_path.display());
    } else {
        warn!("No such file exists at {}", photo_path.display());
    }

    tx.delete_photo(&photo).await?;
    tx.commit().await?;

    Ok(())
}
