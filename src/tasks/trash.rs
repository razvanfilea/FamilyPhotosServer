use crate::http::AppStateRef;
use tokio::fs;
use tracing::{error, info, warn};

pub async fn cleanup_trash(app_state: AppStateRef) -> Result<(), sqlx::Error> {
    for photo in app_state
        .photos_repo
        .get_expired_trash_photos()
        .await?
        .iter()
    {
        let _ = fs::remove_file(
            app_state
                .storage
                .resolve_preview(photo.partial_preview_path()),
        )
        .await;

        let photo_path = app_state.storage.resolve_photo(photo.partial_path());
        if photo_path.exists() {
            fs::remove_file(&photo_path).await.inspect_err(|e| {
                error!("Failed to remove file at {}: {e}", photo_path.display());
            })?;
            info!("Removed trashed file at {}", photo_path.display());
        } else {
            warn!("No such file exists at {}", photo_path.display());
        }

        app_state.photos_repo.delete_photo(photo).await?;
    }

    Ok(())
}
