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
use crate::repo::{FoldersRepo, PhotosHashRepo, PhotosTransactionRepo};
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
    let photo_user_id = (!query.make_public).then_some(user.id);

    let written_file = write_field_to_file(field).await?;

    let mut tx = state.write_pool.begin().await?;
    let photo = tx
        .get_photo_with_hash(&written_file.hash, photo_user_id.as_deref())
        .await?;

    if let Some(photo) = photo {
        let folder_name = tx.as_mut().get_folder_name(photo.folder_id).await?;
        info!(
            "Photo with same hash already exists with path: {}",
            photo.partial_path(folder_name.as_deref())
        );
        return Ok(Json(photo));
    }

    let folder_name = query.folder_name.filter(|s| !s.is_empty());
    let folder_id = tx
        .upsert_folder(photo_user_id.as_deref(), folder_name.as_deref())
        .await?;

    let mut photo = Photo {
        id: 0,
        user_id: photo_user_id,
        name: file_name,
        created_at: query.time_created,
        file_size: written_file.size as i64,
        folder_id,
        thumb_hash: None,
        trashed_on: None,
    };

    let mut photo_path = state
        .storage
        .resolve_photo(photo.partial_path(folder_name.as_deref()));
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
            .resolve_photo(photo.partial_path(folder_name.as_deref()));
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
