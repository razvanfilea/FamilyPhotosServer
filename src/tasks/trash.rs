use crate::http::AppStateRef;
use crate::repo::{FoldersRepo, PhotosRepo, PhotosTransactionRepo};
use tokio::fs;
use tracing::{error, info, warn};

pub async fn cleanup_trash(app_state: AppStateRef) -> Result<(), sqlx::Error> {
    let mut tx = app_state.write_pool.begin().await?;
    let folder_map = tx.as_mut().get_folder_name_map().await?;

    for photo in tx.get_expired_trash_photos().await?.iter() {
        if let Err(e) = fs::remove_file(
            app_state
                .storage
                .resolve_preview(photo.partial_preview_path()),
        )
        .await
        {
            warn!("Failed to remove photo preview: {e}");
        }

        let folder_name = photo.folder_id.and_then(|id| folder_map.get(&id).cloned());
        let photo_path = app_state
            .storage
            .resolve_photo(photo.partial_path(folder_name.as_deref()));
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
