use crate::http::AppStateRef;
use crate::previews::{MIN_PREVIEW_SIZE, generate_thumb_hash_raw_image};
use crate::repo::{PhotosRepo, PhotosTransactionRepo};
use fast_thumbhash::rgba_to_thumb_hash;
use rayon::prelude::*;
use std::path::Path;
use tokio::task::spawn_blocking;
use tracing::{error, info};

const BATCH_SIZE: i64 = 256;

/// Generates thumb hashes in batches, so no database connection is held while reading files.
/// Photos are paginated by id, so a photo without a valid preview is skipped, not retried.
pub async fn generate_thumb_hashes(app_state: AppStateRef) -> Result<(), sqlx::Error> {
    let mut last_id = 0;
    let mut count = 0;

    loop {
        let photos = app_state
            .read_pool
            .get_photos_without_thumb_hash(last_id, BATCH_SIZE)
            .await?;

        let Some(last_photo) = photos.last() else {
            break;
        };
        last_id = last_photo.id;

        let thumb_hashes: Vec<(i64, Vec<u8>)> = spawn_blocking(move || {
            photos
                .par_iter()
                .filter_map(|photo| {
                    let preview_path = app_state
                        .storage
                        .preview_folder
                        .join(photo.partial_preview_path());

                    // Check preview exists and is valid
                    match std::fs::metadata(&preview_path) {
                        Ok(m) if m.len() >= MIN_PREVIEW_SIZE => {}
                        _ => return None,
                    }

                    match generate_thumb_image_hash(&preview_path) {
                        Ok(thumb_hash) => Some((photo.id, thumb_hash)),
                        Err(e) => {
                            error!(
                                "Failed to generate thumb hash for photo {} ({}): {}",
                                photo.name,
                                photo.partial_preview_path(),
                                e
                            );
                            None
                        }
                    }
                })
                .collect()
        })
        .await
        .unwrap_or_else(|e| {
            error!("Thumb hashing batch panicked: {e}");
            Vec::new()
        });

        match save_thumb_hashes(app_state, &thumb_hashes).await {
            Ok(()) => count += thumb_hashes.len(),
            Err(e) => error!(
                "Failed to save a batch of {} thumb hashes: {e}",
                thumb_hashes.len()
            ),
        }
    }

    if count > 0 {
        info!("Updated {count} thumb hashes");
    }

    Ok(())
}

/// Saves a batch in its own short write transaction.
async fn save_thumb_hashes(
    app_state: AppStateRef,
    thumb_hashes: &[(i64, Vec<u8>)],
) -> sqlx::Result<()> {
    let mut tx = app_state.write_pool.begin().await?;
    tx.update_thumb_hashes(thumb_hashes).await?;
    tx.commit().await
}

fn generate_thumb_image_hash(preview_path: &Path) -> Result<Vec<u8>, std::io::Error> {
    let img = generate_thumb_hash_raw_image(preview_path)?;
    Ok(rgba_to_thumb_hash(img.width, img.height, &img.rgba))
}
