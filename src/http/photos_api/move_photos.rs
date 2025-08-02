use crate::http::AppStateRef;
use crate::http::photos_api::check_has_access;
use crate::http::utils::{AuthSession, AxumResult};
use crate::model::photo::Photo;
use crate::model::user::PUBLIC_USER_FOLDER;
use crate::utils::internal_error;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use tracing::{error, info, warn};

pub fn router() -> Router<AppStateRef> {
    Router::new()
        .route("/folder", post(move_folder))
        .route("/{photo_id}", post(move_photo))
}
#[derive(serde::Deserialize)]
struct RenameFolderQuery {
    source_is_public: bool,
    source_folder_name: String,
    target_make_public: bool,
    target_folder_name: Option<String>,
}

async fn move_folder(
    State(state): State<AppStateRef>,
    Query(query): Query<RenameFolderQuery>,
    auth: AuthSession,
) -> AxumResult<impl IntoResponse> {
    let user = auth.user.ok_or(StatusCode::UNAUTHORIZED)?;
    let storage = &state.storage;

    let source_user_name = (!query.source_is_public).then_some(user.id.as_str());
    let target_user_name = (!query.target_make_public).then_some(user.id.as_str());

    let target_folder_name = query.target_folder_name.filter(String::is_empty);

    let photos_to_move = state
        .photos_repo
        .get_photos_in_folder(source_user_name, &query.source_folder_name)
        .await
        .map_err(internal_error)?;

    info!(
        "Moving folder \"{}/{}\" to \"{}/{}\" with {} items",
        source_user_name.unwrap_or(PUBLIC_USER_FOLDER),
        query.source_folder_name,
        target_user_name.unwrap_or(PUBLIC_USER_FOLDER),
        target_folder_name.as_deref().unwrap_or(""),
        photos_to_move.len(),
    );

    let mut moved_photos = Vec::with_capacity(photos_to_move.len());

    for mut photo in photos_to_move {
        let source_path = photo.partial_path();

        photo.user_id = target_user_name.map(ToOwned::to_owned);
        photo.folder = target_folder_name.clone();
        let destination_path = photo.partial_path();

        if let Err(e) = storage.move_photo(&source_path, &destination_path) {
            warn!("Failed to move the photo: {e}");
            continue;
        }

        if let Err(e) = state.photos_repo.update_photo(&photo).await {
            // If the database operation failed for some reason, try to move the image back
            error!("Failed to update the photo: {e}");
            let _ = storage.move_photo(&destination_path, &source_path);
            continue;
        }

        info!("Moved photo from {source_path} to {destination_path}");
        moved_photos.push(photo);
    }

    Ok(Json(moved_photos))
}
#[derive(serde::Deserialize)]
struct MovePhotoQuery {
    make_public: bool,
    target_folder_name: Option<String>,
}

async fn move_photo(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    Query(query): Query<MovePhotoQuery>,
    auth: AuthSession,
) -> AxumResult<impl IntoResponse> {
    let storage = &state.storage;
    let photo = state
        .photos_repo
        .get_photo(photo_id)
        .await
        .map_err(internal_error)?;
    let user = check_has_access(auth.user, &photo)?;

    let target_user_name = (!query.make_public).then_some(user.id);

    let source_path = photo.partial_path();
    let changed_photo = Photo {
        id: photo.id(),
        user_id: target_user_name,
        name: photo.name,
        created_at: photo.created_at,
        file_size: photo.file_size,
        folder: query.target_folder_name,
    };
    let destination_path = changed_photo.partial_path();

    if source_path == destination_path {
        return Err((
            StatusCode::BAD_REQUEST,
            "Source and destination are the same",
        )
            .into_response()
            .into());
    }

    info!("Moving photo from {source_path} to {destination_path}");

    storage
        .move_photo(&source_path, &destination_path)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed moving the photo: {e}"),
            )
        })?;

    state
        .photos_repo
        .update_photo(&changed_photo)
        .await
        .inspect_err(|e| {
            error!("Failed to update photo: {e}");
            // Try to undo the move
            let _ = storage.move_photo(&destination_path, &source_path);
        })
        .map_err(internal_error)?;

    Ok(Json(changed_photo))
}
