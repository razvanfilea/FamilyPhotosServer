use crate::http::AppStateRef;
use crate::repo::{PhotosRepo, PhotosTransactionRepo};
use tokio::fs;
use tracing::{error, info, warn};

pub async fn cleanup_trash(app_state: AppStateRef) -> Result<(), sqlx::Error> {
    let mut tx = app_state.pool.begin().await?;

    for photo in tx.get_expired_trash_photos().await?.iter() {
        let _ = fs::remove_file(
            app_state
                .storage
                .resolve_preview(photo.partial_preview_path()),
        )
        .await;

        let photo_path = app_state.storage.resolve_photo(photo.partial_path());
        let display_path = photo_path.display();
        if photo_path.exists() {
            info!("Removing trashed file at {}", display_path);
            fs::remove_file(&photo_path).await.inspect_err(|e| {
                error!("Failed to remove file at {}: {e}", display_path);
            })?;
            info!("Removed trashed file at {}", display_path);
        } else {
            warn!("No such file exists at {}", display_path);
        }

        tx.delete_photo(photo).await?;
    }

    tx.commit().await
}
