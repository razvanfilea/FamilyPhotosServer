mod favorite;

use std::string::ToString;

use axum::response::ErrorResponse;
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

use crate::http::AppState;
use crate::http::utils::status_error::StatusError;
use crate::http::utils::{AuthSession, AxumResult, file_to_response, write_field_to_file};
use crate::model::photo::{Photo, PhotoBase, PhotoBody};
use crate::model::user::{PUBLIC_USER_ID, User};
use crate::previews;
use crate::utils::{internal_error, read_exif};
use time::serde::timestamp;

pub fn router(app_state: AppState) -> Router {
    Router::new()
        .route("/", get(photos_list))
        .route("/download/{photo_id}", get(download_photo))
        .route("/preview/{photo_id}", get(preview_photo))
        .route("/exif/{photo_id}", get(get_photo_exif))
        .route("/upload", post(upload_photo))
        .route("/delete/{photo_id}", delete(delete_photo))
        .route("/change_location/{photo_id}", post(change_photo_location))
        .route("/rename_folder", post(rename_folder))
        .nest("/favorite", favorite::router())
        .with_state(app_state)
}

fn check_has_access(user: Option<User>, photo: &Photo) -> Result<User, ErrorResponse> {
    let user = user.ok_or(StatusCode::UNAUTHORIZED)?;

    if photo.user_id() == &user.id || photo.user_id() == PUBLIC_USER_ID {
        Ok(user)
    } else {
        Err(StatusError::new_status(
            "You don't have access to this resource",
            StatusCode::FORBIDDEN,
        ))
    }
}

async fn photos_list(
    State(state): State<AppState>,
    auth: AuthSession,
) -> AxumResult<impl IntoResponse> {
    let user = auth.user.ok_or(StatusCode::BAD_REQUEST)?;

    Ok(Json(
        state
            .photos_repo
            .get_photos_by_user_and_public(user.id)
            .await?,
    ))
}

async fn preview_photo(
    State(state): State<AppState>,
    Path(photo_id): Path<i64>,
    auth: AuthSession,
) -> impl IntoResponse {
    let AppState {
        storage,
        users_repo: _users_repo,
        photos_repo,
    } = state;

    let photo = photos_repo.get_photo(photo_id).await?;
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
    State(state): State<AppState>,
    Path(photo_id): Path<i64>,
    auth: AuthSession,
) -> impl IntoResponse {
    let photo = state.photos_repo.get_photo(photo_id).await?;
    check_has_access(auth.user, &photo)?;

    let photo_path = state.storage.resolve_photo(photo.partial_path());

    file_to_response(&photo_path).await
}

async fn get_photo_exif(
    State(state): State<AppState>,
    Path(photo_id): Path<i64>,
    auth: AuthSession,
) -> impl IntoResponse {
    let photo = state.photos_repo.get_photo(photo_id).await?;
    check_has_access(auth.user, &photo)?;

    let path = state.storage.resolve_photo(photo.partial_path());
    let exif = task::spawn_blocking(move || read_exif(path))
        .await
        .map_err(internal_error)?;

    match exif {
        Some(exif) => Ok(Json(exif)),
        None => Err(StatusError::new_status(
            "Exif data not found",
            StatusCode::NOT_FOUND,
        )),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadDataQuery {
    #[serde(with = "timestamp")]
    time_created: OffsetDateTime,
    folder_name: Option<String>,
    #[serde(default)]
    make_public: bool,
}

async fn upload_photo(
    State(state): State<AppState>,
    Query(query): Query<UploadDataQuery>,
    auth: AuthSession,
    mut payload: Multipart,
) -> AxumResult<impl IntoResponse> {
    let user = auth.user.ok_or(StatusCode::UNAUTHORIZED)?;

    let field = payload
        .next_field()
        .await?
        .ok_or_else(|| StatusError::new_status("Multipart is empty", StatusCode::BAD_REQUEST))?;

    let file_name = field
        .file_name()
        .or(field.name())
        .ok_or_else(|| StatusError::new_status("Multipart has no name", StatusCode::BAD_REQUEST))?;

    let mut new_photo_body = PhotoBody::new(
        if query.make_public {
            String::from(PUBLIC_USER_ID)
        } else {
            user.id
        },
        String::from(file_name),
        query.time_created,
        0, // To be set after it is written to disk
        query.folder_name,
    );

    let photo_path = state.storage.resolve_photo(new_photo_body.partial_path());
    if let Some(parent) = photo_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).await.map_err(internal_error)?;
        }
    }

    info!("Uploading file to {}", photo_path.display());

    match write_field_to_file(field, &photo_path).await {
        Ok(file_size) => new_photo_body.set_file_size(file_size as i64),
        Err(e) => {
            // Upload failed, delete the file
            let _ = fs::remove_file(photo_path).await;
            return Err(e);
        }
    }

    match state.photos_repo.insert_photo(&new_photo_body).await {
        Ok(photo) => Ok(Json(photo)),
        Err(e) => {
            // Insertion failed, delete the file
            let _ = fs::remove_file(photo_path).await;
            Err(e)
        }
    }
}

async fn delete_photo(
    State(state): State<AppState>,
    Path(photo_id): Path<i64>,
    auth: AuthSession,
) -> AxumResult<impl IntoResponse> {
    let photo = state.photos_repo.get_photo(photo_id).await?;
    check_has_access(auth.user, &photo)?;

    let _ = fs::remove_file(state.storage.resolve_preview(photo.partial_preview_path())).await;

    let photo_path = state.storage.resolve_photo(photo.partial_path());
    if photo_path.exists() {
        fs::remove_file(photo_path)
            .await
            .map_err(|e| StatusError::create(format!("Failed to delete file: {e}")))?;
    }

    state.photos_repo.delete_photo(photo_id).await?;

    Ok(())
}

#[derive(serde::Deserialize)]
struct ChangeLocationQuery {
    make_public: bool,
    target_folder_name: Option<String>,
}

async fn change_photo_location(
    State(state): State<AppState>,
    Path(photo_id): Path<i64>,
    Query(query): Query<ChangeLocationQuery>,
    auth: AuthSession,
) -> AxumResult<impl IntoResponse> {
    let storage = state.storage;
    let photo = state.photos_repo.get_photo(photo_id).await?;
    let user = check_has_access(auth.user, &photo)?;

    let target_user_name = if query.make_public {
        PUBLIC_USER_ID.to_string()
    } else {
        user.id
    };

    let changed_photo = Photo {
        id: photo.id(),
        user_id: target_user_name,
        name: photo.name().clone(),
        created_at: photo.created_at(),
        file_size: photo.file_size(),
        folder: query.target_folder_name.clone(),
    };

    let source_path = photo.partial_path();
    let destination_path = changed_photo.partial_path();

    if source_path == destination_path {
        return Err(StatusError::new_status(
            "Source and destination are the same",
            StatusCode::BAD_REQUEST,
        ));
    }

    info!("Moving photo from {source_path} to {destination_path}");

    storage
        .move_photo(&source_path, &destination_path)
        .map_err(|e| StatusError::create(format!("Failed moving the photo: {e}")))?;

    state
        .photos_repo
        .update_photo(&changed_photo)
        .await
        .map_err(|_| StatusError::create("Something went wrong moving the photo"))?;

    Ok(Json(changed_photo))
}

#[derive(serde::Deserialize)]
struct RenameFolderQuery {
    source_is_public: bool,
    source_folder_name: String,
    target_make_public: bool,
    target_folder_name: Option<String>,
}

async fn rename_folder(
    State(state): State<AppState>,
    Query(query): Query<RenameFolderQuery>,
    auth: AuthSession,
) -> AxumResult<impl IntoResponse> {
    let user = auth.user.expect("User should be logged in");
    let storage = state.storage;

    let source_user_name = if query.source_is_public {
        PUBLIC_USER_ID
    } else {
        &user.id
    };
    let target_user_name = if query.target_make_public {
        PUBLIC_USER_ID
    } else {
        &user.id
    };

    let photos_to_move = state
        .photos_repo
        .get_photos_in_folder(source_user_name, query.source_folder_name)
        .await?;
    let mut moved_photos = Vec::with_capacity(photos_to_move.len());

    for mut photo in photos_to_move {
        let source_path = photo.partial_path();

        photo.user_id = target_user_name.to_owned();
        photo.folder = query.target_folder_name.clone();
        let destination_path = photo.partial_path();

        if let Err(e) = storage.move_photo(&source_path, &destination_path) {
            warn!("Failed to move the photo: {e}");
            continue;
        }

        if let Err(e) = state.photos_repo.update_photo(&photo).await {
            // If the database operation failed for some reason, try to move the image back
            error!("Failed to update the photo: {e:?}");
            let _ = storage.move_photo(&destination_path, &source_path);
        }

        moved_photos.push(photo);
    }

    Ok(Json(moved_photos))
}
