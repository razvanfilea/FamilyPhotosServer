use crate::http::AppStateRef;
use crate::previews::{MIN_PREVIEW_SIZE, generate_thumb_hash_raw_image};
use crate::repo::{PhotosRepo, PhotosTransactionRepo};
use fast_thumbhash::rgba_to_thumb_hash;
use rayon::prelude::*;
use std::path::Path;
use tokio::task::spawn_blocking;
use tracing::{error, info};

pub async fn generate_thumb_hashes(app_state: AppStateRef) -> Result<(), sqlx::Error> {
    const CHUNK_SIZE: usize = 256;

    let mut tx = app_state.write_pool.begin().await?;
    let photos = tx.get_photos_without_thumb_hash().await?;

    info!("Computing thumb hashes for {} photos", photos.len());

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

    spawn_blocking(move || {
        photos.par_chunks(CHUNK_SIZE).for_each(|chunk| {
            let chunk = chunk
                .iter()
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
                                photo.full_name(),
                                photo.partial_preview_path(),
                                e
                            );
                            None
                        }
                    }
                })
                .collect::<Vec<_>>();

            if let Err(e) = sender.send(chunk) {
                error!("Failed to send thumb hashes over channel: {e}");
            }
        });

        drop(sender);
    });

    let mut count = 0;

    while let Some(chunk) = receiver.recv().await {
        tx.update_thumb_hashes(&chunk).await?;
        count += chunk.len();
    }

    tx.commit().await?;

    info!("Updated {count} thumb hashes");

    Ok(())
}

fn generate_thumb_image_hash(preview_path: &Path) -> Result<Vec<u8>, std::io::Error> {
    let img = generate_thumb_hash_raw_image(preview_path)?;
    Ok(rgba_to_thumb_hash(img.width, img.height, &img.rgba))
}
