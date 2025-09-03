mod favorite;
mod move_photos;
mod sync;
mod trash;

use axum::{
    Json, Router,
    extract::Multipart,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
};
use time::OffsetDateTime;
use tokio::{fs, task};
use tracing::{error, info, warn};

use crate::http::AppStateRef;
use crate::http::utils::{AuthSession, AxumResult, file_to_response, write_field_to_file};
use crate::model::photo::Photo;
use crate::previews;
use crate::utils::exif::read_exif;
use crate::utils::internal_error;
use time::serde::timestamp;

pub fn router(app_state: AppStateRef) -> Router {
    Router::new()
        .nest("/sync", sync::router())
        .nest("/move", move_photos::router())
        .nest("/trash", trash::router())
        .route("/duplicates", get(get_duplicates))
        .route("/download/{photo_id}", get(download_photo))
        .route("/preview/{photo_id}", get(preview_photo))
        .route("/exif/{photo_id}", get(get_photo_exif))
        .route("/upload", post(upload_photo))
        .route("/delete/{photo_id}", delete(delete_photo))
        .nest("/favorite", favorite::router())
        .with_state(app_state)
}

async fn get_duplicates(
    State(state): State<AppStateRef>,
    auth: AuthSession,
) -> AxumResult<impl IntoResponse> {
    let user = auth.user.ok_or(StatusCode::UNAUTHORIZED)?;

    let photos = state
        .photo_hash_repo
        .get_duplicates_for_user(user.id.as_str())
        .await
        .map_err(internal_error)?;

    Ok(Json(photos))
}

async fn preview_photo(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    auth: AuthSession,
) -> impl IntoResponse {
    let user = auth.user.ok_or(StatusCode::UNAUTHORIZED)?;
    let storage = &state.storage;
    let photos_repo = &state.photos_repo;

    let photo = photos_repo
        .get_photo(photo_id, &user.id)
        .await
        .map_err(internal_error)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let photo_path = storage.resolve_photo(photo.partial_path());
    let preview_path = storage.resolve_preview(photo.partial_preview_path());

    let preview_generated = if !preview_path.exists() {
        let photo_path_clone = photo_path.clone();
        let preview_path_clone = preview_path.clone();

        task::spawn_blocking(move || {
            previews::generate_preview(photo_path_clone, preview_path_clone)
        })
        .await
        .map_err(internal_error)?
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

    file_to_response(&path).await
}

async fn download_photo(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    auth: AuthSession,
) -> impl IntoResponse {
    let user = auth.user.ok_or(StatusCode::UNAUTHORIZED)?;
    let photo = state
        .photos_repo
        .get_photo(photo_id, &user.id)
        .await
        .map_err(internal_error)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let photo_path = state.storage.resolve_photo(photo.partial_path());

    file_to_response(&photo_path).await
}

async fn get_photo_exif(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    auth: AuthSession,
) -> AxumResult<impl IntoResponse> {
    let user = auth.user.ok_or(StatusCode::UNAUTHORIZED)?;
    let photo = state
        .photos_repo
        .get_photo(photo_id, &user.id)
        .await
        .map_err(internal_error)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let path = state.storage.resolve_photo(photo.partial_path());
    let exif = task::spawn_blocking(move || read_exif(path))
        .await
        .map_err(internal_error)?;

    match exif {
        Some(exif) => Ok(Json(exif).into_response()),
        None => Ok((StatusCode::NOT_FOUND, "Exif data not found").into_response()),
    }
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
    auth: AuthSession,
    mut payload: Multipart,
) -> AxumResult<impl IntoResponse> {
    let user = auth.user.ok_or(StatusCode::UNAUTHORIZED)?;

    let field = payload
        .next_field()
        .await?
        .ok_or((StatusCode::BAD_REQUEST, "Multipart is empty"))?;

    let file_name = field
        .file_name()
        .or(field.name())
        .ok_or((StatusCode::BAD_REQUEST, "Multipart has no name"))?
        .to_owned();
    let photo_user_id = (!query.make_public).then_some(user.id);

    let written_file = write_field_to_file(field).await?;

    let photo = state
        .photo_hash_repo
        .get_photo_with_hash(&written_file.hash, photo_user_id.as_deref())
        .await
        .map_err(internal_error)?;

    if let Some(photo) = photo {
        info!(
            "Photo with same hash already exists with path: {}",
            photo.partial_path()
        );
        return Ok(Json(photo));
    }

    let mut new_photo = Photo {
        id: 0,
        user_id: photo_user_id,
        name: file_name,
        created_at: query.time_created,
        file_size: written_file.size as i64,
        folder: query.folder_name,
        thumb_hash: None,
        trashed_on: None,
    };

    let mut photo_path = state.storage.resolve_photo(new_photo.partial_path());
    if let Some(parent) = photo_path.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent).await.map_err(internal_error)?;
    }

    // If the file exists, generate a random name
    if photo_path.exists() {
        new_photo.name = format!(
            "{}.{}",
            uuid::Uuid::new_v4(),
            photo_path
                .extension()
                .map(|str| str.to_string_lossy().to_string())
                .unwrap_or_default()
        );
        photo_path = state.storage.resolve_photo(new_photo.partial_path());
    }

    info!("Uploading file to {}", photo_path.display());

    written_file.persist_to(&photo_path).await.map_err(|e| {
        error!("Failed to move temporary file to final location: {e}");
        (StatusCode::INTERNAL_SERVER_ERROR, "Failed to save file")
    })?;

    match state.photos_repo.insert_photo(&new_photo).await {
        Ok(photo) => Ok(Json(photo)),
        Err(e) => {
            // Insertion failed, delete the file
            let _ = fs::remove_file(photo_path).await;
            Err(internal_error(e))
        }
    }
}

async fn delete_photo(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    auth: AuthSession,
) -> AxumResult<impl IntoResponse> {
    let user = auth.user.ok_or(StatusCode::UNAUTHORIZED)?;
    let photo = state
        .photos_repo
        .get_photo(photo_id, &user.id)
        .await
        .map_err(internal_error)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let _ = fs::remove_file(state.storage.resolve_preview(photo.partial_preview_path())).await;

    let photo_path = state.storage.resolve_photo(photo.partial_path());
    if photo_path.exists() {
        fs::remove_file(&photo_path).await.map_err(|e| {
            error!("Failed to remove file at {}: {e}", photo_path.display());
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to delete file: {e}"),
            )
        })?;
        info!("Removed file at {}", photo_path.display());
    } else {
        warn!("No such file exists at {}", photo_path.display());
    }

    state
        .photos_repo
        .delete_photo(&photo)
        .await
        .map_err(internal_error)?;

    Ok(())
}
