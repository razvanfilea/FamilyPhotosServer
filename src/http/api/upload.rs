use axum::{
    Json, Router,
    extract::{Multipart, Query, State},
    response::IntoResponse,
    routing::post,
};
use time::OffsetDateTime;
use tokio::fs;
use tracing::{error, info};

use crate::http::error::{HttpError, HttpResult};
use crate::http::utils::write_field_to_file;
use crate::http::{AppStateRef, auth::AuthenticatedUser};
use crate::model::photo::Photo;
use crate::repo::{FolderPermissionsRepo, FoldersRepo, PhotosHashRepo, PhotosTransactionRepo};
use sqlx::{Sqlite, Transaction};
use time::serde::timestamp;

pub fn router() -> Router<AppStateRef> {
    Router::new().route("/upload", post(upload_photo))
}

#[derive(Debug, serde::Deserialize)]
struct UploadDataQuery {
    #[serde(with = "timestamp")]
    time_created: OffsetDateTime,
    folder_name: Option<String>,
    #[serde(default)]
    make_public: bool,
    folder_id: Option<i64>,
}

struct UploadTarget {
    photo_user_id: Option<String>,
    folder_id: Option<i64>,
    folder_name: Option<String>,
}

async fn resolve_upload_target(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    query: &UploadDataQuery,
) -> HttpResult<UploadTarget> {
    let Some(target_folder_id) = query.folder_id else {
        return Ok(UploadTarget {
            photo_user_id: (!query.make_public).then_some(user_id.to_owned()),
            folder_id: None,
            folder_name: query.folder_name.clone().filter(|s| !s.is_empty()),
        });
    };

    let folder = tx
        .as_mut()
        .get_folder(target_folder_id)
        .await?
        .ok_or(HttpError::NotFound)?;

    let is_owner = folder.owner_id.as_deref() == Some(user_id);
    let is_public = folder.owner_id.is_none();

    if !is_owner && !is_public {
        let permission = tx
            .as_mut()
            .get_grantee_permission(user_id, target_folder_id)
            .await?
            .ok_or(HttpError::NotFound)?;

        if permission.is_expired() || !permission.can_upload {
            return Err(HttpError::NotFound);
        }
    }

    Ok(UploadTarget {
        photo_user_id: folder.owner_id,
        folder_id: Some(target_folder_id),
        folder_name: Some(folder.name),
    })
}

async fn upload_photo(
    State(state): State<AppStateRef>,
    Query(query): Query<UploadDataQuery>,
    AuthenticatedUser(user): AuthenticatedUser,
    mut payload: Multipart,
) -> HttpResult<impl IntoResponse> {
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

    let mut tx = state.write_pool.begin().await?;

    let target = resolve_upload_target(&mut tx, &user.id, &query).await?;

    let written_file = write_field_to_file(field).await?;

    let existing = tx
        .get_photo_with_hash(&written_file.hash, target.photo_user_id.as_deref())
        .await?;

    if let Some(photo) = existing {
        let folder_name = tx.as_mut().get_folder_name(photo.folder_id).await?;
        info!(
            "Photo with same hash already exists with path: {}",
            photo.partial_path(folder_name.as_deref())
        );
        return Ok(Json(photo));
    }

    let folder_id = match target.folder_id {
        Some(id) => Some(id),
        None => {
            tx.upsert_folder(
                target.photo_user_id.as_deref(),
                target.folder_name.as_deref(),
            )
            .await?
        }
    };

    let mut photo = Photo {
        id: 0,
        user_id: target.photo_user_id,
        name: file_name,
        created_at: query.time_created,
        file_size: written_file.size as i64,
        folder_id,
        thumb_hash: None,
        trashed_on: None,
    };

    let mut photo_path = state
        .storage
        .resolve_photo(photo.partial_path(target.folder_name.as_deref()));
    if let Some(parent) = photo_path.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent).await?;
    }

    // If the file exists, generate a random name
    if photo_path.exists() {
        photo.name = format!(
            "{}.{}",
            uuid::Uuid::new_v4(),
            photo_path
                .extension()
                .map(|str| str.to_string_lossy().to_string())
                .unwrap_or_default()
        );
        photo_path = state
            .storage
            .resolve_photo(photo.partial_path(target.folder_name.as_deref()));
    }

    info!("Uploading file to {}", photo_path.display());

    let photo = tx.insert_photo(&photo).await?;

    written_file.persist_to(&photo_path).await?;

    tx.commit().await.inspect_err(|_| {
        // Transaction failed, delete the file
        if let Err(e) = std::fs::remove_file(photo_path) {
            error!("Failed to remove uploaded file: {e}");
        }
    })?;

    Ok(Json(photo))
}
