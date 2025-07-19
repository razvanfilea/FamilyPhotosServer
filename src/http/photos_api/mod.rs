mod favorite;
mod sync;
mod move_photos;

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
use crate::model::user::{PUBLIC_USER_ID, User};
use crate::previews;
use crate::utils::exif::read_exif;
use crate::utils::internal_error;
use time::serde::timestamp;

pub fn router(app_state: AppStateRef) -> Router {
    Router::new()
        .nest("/sync", sync::router())
        .nest("/move", move_photos::router())
        .route("/duplicates", get(get_duplicates))
        .route("/download/{photo_id}", get(download_photo))
        .route("/preview/{photo_id}", get(preview_photo))
        .route("/exif/{photo_id}", get(get_photo_exif))
        .route("/upload", post(upload_photo))
        .route("/delete/{photo_id}", delete(delete_photo))
        .nest("/favorite", favorite::router())
        .with_state(app_state)
}

fn check_has_access(user: Option<User>, photo: &Photo) -> Result<User, StatusCode> {
    let user = user.ok_or(StatusCode::UNAUTHORIZED)?;

    if photo.user_id == user.id || photo.user_id == PUBLIC_USER_ID {
        Ok(user)
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

async fn get_duplicates(
    State(state): State<AppStateRef>,
    auth: AuthSession,
) -> AxumResult<impl IntoResponse> {
    let user = auth.user.ok_or(StatusCode::BAD_REQUEST)?;

    let photos = state
        .photo_hash_repo
        .get_duplicates_for_user(user.id)
        .await
        .map_err(internal_error)?;

    Ok(Json(photos))
}

async fn preview_photo(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    auth: AuthSession,
) -> impl IntoResponse {
    let storage = &state.storage;
    let photos_repo = &state.photos_repo;

    let photo = photos_repo
        .get_photo(photo_id)
        .await
        .map_err(internal_error)?;
    check_has_access(auth.user, &photo)?;

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
                "Preview generation failed for video: {}\nCause: {e}",
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
    let photo = state
        .photos_repo
        .get_photo(photo_id)
        .await
        .map_err(internal_error)?;
    check_has_access(auth.user, &photo)?;

    let photo_path = state.storage.resolve_photo(photo.partial_path());

    file_to_response(&photo_path).await
}

async fn get_photo_exif(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    auth: AuthSession,
) -> AxumResult<impl IntoResponse> {
    let photo = state
        .photos_repo
        .get_photo(photo_id)
        .await
        .map_err(internal_error)?;
    check_has_access(auth.user, &photo)?;

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
        .ok_or((StatusCode::UNPROCESSABLE_ENTITY, "Multipart is empty"))?;

    let file_name = field
        .file_name()
        .or(field.name())
        .ok_or((StatusCode::BAD_REQUEST, "Multipart has no name"))?;

    let mut new_photo = Photo {
        id: 0,
        user_id: if query.make_public {
            String::from(PUBLIC_USER_ID)
        } else {
            user.id
        },
        name: String::from(file_name),
        created_at: query.time_created,
        file_size: 0, // To be set after it is written to disk
        folder: query.folder_name,
    };

    let photo_path = state.storage.resolve_photo(new_photo.partial_path());
    if let Some(parent) = photo_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).await.map_err(internal_error)?;
        }
    }

    info!("Uploading file to {}", photo_path.display());

    match write_field_to_file(field, &photo_path).await {
        Ok(file_size) => new_photo.file_size = file_size as i64,
        Err(e) => {
            // Upload failed, delete the file
            let _ = fs::remove_file(photo_path).await;
            return Err(e);
        }
    }

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
    let photo = state
        .photos_repo
        .get_photo(photo_id)
        .await
        .map_err(internal_error)?;
    check_has_access(auth.user, &photo)?;

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
