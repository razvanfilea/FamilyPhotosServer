use crate::http::AppStateRef;
use crate::http::error::{HttpError, HttpResult};
use crate::http::utils::AuthSession;
use crate::model::photo::Photo;
use crate::repo::{PhotosRepo, PhotosTransactionRepo};
use crate::utils::storage_resolver::StorageResolver;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use std::path::{Path as StdPath, PathBuf};
use tokio::fs;
use tokio::process::Command;
use tracing::{error, info};

pub fn router() -> Router<AppStateRef> {
    Router::new().route("/{photo_id}", post(reencode_video))
}

async fn reencode_video(
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    auth_session: AuthSession,
) -> HttpResult<impl IntoResponse> {
    let user = auth_session.user.ok_or(HttpError::Unauthorized)?;

    let photo = state
        .read_pool
        .get_photo(photo_id, &user.id)
        .await?
        .ok_or(HttpError::NotFound)?;

    let input_path = state.storage.resolve_photo(photo.partial_path());
    if !input_path.exists() {
        return Err(HttpError::NotFound);
    }

    let (final_photo, final_output_path) = resolve_final_paths(&photo, &state.storage);

    let temp_uuid = uuid::Uuid::new_v4().simple();
    let temp_output_path = input_path.with_extension(format!("{temp_uuid}.reencoding.mp4"));
    let backup_path = input_path.with_extension(format!("{temp_uuid}.bak"));

    // Run ffmpeg
    encode_video_to_hevc(&input_path, &temp_output_path).await?;

    let new_size = fs::metadata(&temp_output_path).await?.len();
    if new_size >= photo.file_size as u64 {
        info!(
            "Re-encoded video for photo {} is not smaller ({} >= {}). Keeping original.",
            photo.id, new_size, photo.file_size
        );
        let _ = fs::remove_file(&temp_output_path).await;
        return Err(HttpError::BadRequest(
            "Re-encoded video is not smaller than original".to_string(),
        ));
    }

    // Perform atomic update
    let updated_photo = perform_atomic_update(
        &state,
        photo,
        final_photo.name,
        input_path,
        final_output_path,
        temp_output_path,
        backup_path,
    )
    .await?;

    Ok(Json(updated_photo))
}

fn resolve_final_paths(photo: &Photo, storage: &StorageResolver) -> (Photo, PathBuf) {
    let mut output_name = PathBuf::from(&photo.name);
    output_name.set_extension("mp4");

    let mut final_photo = photo.clone();
    final_photo.name = output_name.to_string_lossy().to_string();
    let mut final_output_path = storage.resolve_photo(final_photo.partial_path());

    let input_path = storage.resolve_photo(photo.partial_path());

    // If the target path exists and it's NOT our current file, find a new name
    if final_output_path.exists() && final_output_path != input_path {
        let stem = output_name
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        final_photo.name = format!("{}-{}.mp4", stem, uuid::Uuid::new_v4().simple());
        final_output_path = storage.resolve_photo(final_photo.partial_path());
    }

    (final_photo, final_output_path)
}

async fn encode_video_to_hevc(input_path: &StdPath, output_path: &StdPath) -> HttpResult<()> {
    info!("Re-encoding video {} to HEVC", input_path.display());

    let status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(input_path)
        .arg("-c:v")
        .arg("libx265")
        .arg("-crf")
        .arg("26")
        .arg("-preset")
        .arg("medium")
        .arg("-c:a")
        .arg("aac")
        .arg("-b:a")
        .arg("128k")
        .arg(output_path)
        .status()
        .await
        .map_err(|e| HttpError::AnyError(Box::new(e)))?;

    if !status.success() {
        error!("ffmpeg failed with status: {status}");
        let _ = fs::remove_file(output_path).await;
        return Err(HttpError::BadRequest("ffmpeg re-encoding failed".to_string()));
    }

    Ok(())
}

async fn perform_atomic_update(
    state: &AppStateRef,
    photo: Photo,
    new_name: String,
    input_path: PathBuf,
    final_output_path: PathBuf,
    temp_output_path: PathBuf,
    backup_path: PathBuf,
) -> HttpResult<Photo> {
    // 1. Move original to backup
    fs::rename(&input_path, &backup_path).await?;

    // 2. Ensure destination directory exists and move temp output to final output
    if let Some(parent) = final_output_path.parent() {
        fs::create_dir_all(parent).await?;
    }

    if let Err(e) = fs::rename(&temp_output_path, &final_output_path).await {
        error!("Failed to move re-encoded file to final destination: {e}");
        // Rollback: move backup back to original
        let _ = fs::rename(&backup_path, &input_path).await;
        let _ = fs::remove_file(&temp_output_path).await;
        return Err(HttpError::AnyError(Box::new(e)));
    }

    // Helper for DB-related rollback
    let rollback_fs = || async {
        let _ = fs::remove_file(&final_output_path).await;
        let _ = fs::rename(&backup_path, &input_path).await;
    };

    // 3. Update DB
    let new_size = match fs::metadata(&final_output_path).await {
        Ok(m) => m.len(),
        Err(e) => {
            rollback_fs().await;
            return Err(HttpError::AnyError(Box::new(e)));
        }
    };

    let mut tx = state.write_pool.begin().await?;

    let mut updated_photo = photo.clone();
    updated_photo.name = new_name;
    updated_photo.file_size = new_size as i64;

    if let Err(e) = tx.update_photo(&updated_photo).await {
        error!("Failed to update photo in DB: {e}");
        rollback_fs().await;
        return Err(HttpError::Database(e));
    }

    if let Err(e) = tx.commit().await {
        error!("Failed to commit transaction: {e}");
        rollback_fs().await;
        return Err(HttpError::Database(e));
    }

    // 4. Cleanup backup
    let _ = fs::remove_file(&backup_path).await;

    info!(
        "Successfully re-encoded video {} to {}",
        photo.id, updated_photo.name
    );

    Ok(updated_photo)
}
