use axum::{
    Json, Router,
    extract::{Multipart, Query, State},
    response::IntoResponse,
    routing::post,
};
use time::OffsetDateTime;
use tokio::fs;
use tracing::{error, info};

use crate::{http::utils::write_field_to_file, model::photo_hash::PhotoHash, utils::crop_blake_3_hash};
use crate::http::{AppStateRef, auth::AuthenticatedUser};
use crate::model::photo::Photo;
use crate::repo::{FoldersRepo, PhotosHashRepo, PhotosTransactionRepo};
use crate::{
    http::error::{HttpError, HttpResult},
    model::folder::AccessibleFolder,
};
use sqlx::{Sqlite, Transaction};
use time::serde::timestamp;

pub fn router() -> Router<AppStateRef> {
    Router::new().route("/", post(upload_photo))
}

#[derive(Debug, serde::Deserialize)]
struct UploadDataQuery {
    #[serde(with = "timestamp")]
    time_created: OffsetDateTime,
    folder_id: Option<i64>,
    #[serde(default)]
    make_public: bool,
    hash: Option<String>,
}

struct UploadTarget {
    photo_user_id: Option<String>,
    folder: Option<AccessibleFolder>,
}

async fn resolve_upload_target(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    query: &UploadDataQuery,
) -> HttpResult<UploadTarget> {
    let Some(target_folder_id) = query.folder_id else {
        return Ok(UploadTarget {
            photo_user_id: (!query.make_public).then_some(user_id.to_owned()),
            folder: None,
        });
    };

    let folder = tx
        .as_mut()
        .get_accessible_folder(user_id, target_folder_id)
        .await?
        .ok_or(HttpError::Unauthorized)?;

    if !folder.can_upload {
        return Err(HttpError::Unauthorized);
    }

    Ok(UploadTarget {
        photo_user_id: folder.owner_id.clone(),
        folder: Some(folder),
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

    // Check the hash before writing the file
    if let Ok(client_hash) = blake3::Hash::from_hex(query.hash.as_deref().unwrap_or("")) {
        let hash = crop_blake_3_hash(client_hash.as_bytes());
        if let Some(photo) = check_existing_photo_with_hash(&mut tx, &target, &hash).await? {
            return Ok(Json(photo));
        }
    }

    let written_file = write_field_to_file(field).await?;

    if let Some(photo) =
        check_existing_photo_with_hash(&mut tx, &target, &written_file.hash).await?
    {
        return Ok(Json(photo));
    }

    let folder_name = target.folder.as_ref().map(|f| f.name.as_str());

    let mut photo = Photo {
        id: 0,
        user_id: target.photo_user_id,
        name: file_name,
        created_at: query.time_created,
        file_size: written_file.size as i64,
        folder_id: target.folder.as_ref().map(|f| f.id),
        thumb_hash: None,
        trashed_on: None,
    };

    let mut photo_path = state.storage.resolve_photo(photo.partial_path(folder_name));
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
        photo_path = state.storage.resolve_photo(photo.partial_path(folder_name));
    }

    info!("Uploading file to {}", photo_path.display());

    let photo = tx.insert_photo(&photo).await?;
    tx.insert_hash(PhotoHash{id: photo.id, hash: written_file.hash.clone()}).await?;

    written_file.persist_to(&photo_path).await?;

    tx.commit().await.inspect_err(|_| {
        // Transaction failed, delete the file
        if let Err(e) = std::fs::remove_file(photo_path) {
            error!("Failed to remove uploaded file: {e}");
        }
    })?;

    Ok(Json(photo))
}

async fn check_existing_photo_with_hash(
    tx: &mut Transaction<'_, Sqlite>,
    target: &UploadTarget,
    hash: &[u8],
) -> Result<Option<Photo>, HttpError> {
    let existing = tx
        .get_photo_with_hash(hash, target.photo_user_id.as_deref())
        .await?;

    if let Some(photo) = existing {
        let folder_name = tx.as_mut().get_folder_name(photo.folder_id).await?;
        info!(
            "Photo with same hash already exists with path: {}",
            photo.partial_path(folder_name.as_deref())
        );
        return Ok(Some(photo));
    }

    Ok(None)
}
