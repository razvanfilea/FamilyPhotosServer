use crate::http::AppStateRef;
use crate::previews::{THUMB_HASH_IMAGE_SIZE, generate_thumb_hash_raw_image};
use crate::repo::{PhotosRepo, PhotosTransactionRepo};
use fast_thumbhash::ThumbHashEncoder;
use rayon::prelude::*;
use std::path::Path;
use tokio::task::spawn_blocking;
use tracing::{error, info};

pub async fn generate_thumb_hashes(app_state: AppStateRef) -> Result<(), sqlx::Error> {
    const CHUNK_SIZE: usize = 256;

    let mut tx = app_state.pool.begin().await?;
    let photos = tx.get_photos_without_thumb_hash().await?;

    info!("Computing thumb hashes for {} photos", photos.len());

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

    spawn_blocking(move || {
        photos.par_chunks(CHUNK_SIZE).for_each(|chunk| {
            let Ok(mut encoder) =
                ThumbHashEncoder::new(THUMB_HASH_IMAGE_SIZE, THUMB_HASH_IMAGE_SIZE)
                    .inspect_err(|e| error!("Failed to create thumb hash encoder: {}", e))
            else {
                return;
            };

            let chunk = chunk
                .iter()
                .filter_map(|photo| {
                    let preview_path = app_state
                        .storage
                        .preview_folder
                        .join(photo.partial_preview_path());

                    if !preview_path.exists() {
                        return None;
                    }

                    match generate_thumb_image_hash(&mut encoder, &preview_path) {
                        Ok(thumb_hash) => Some((photo.id, thumb_hash)),
                        Err(e) => {
                            error!(
                                "Failed to generate thumb hash for photo {}: {}",
                                photo.full_name(),
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

    info!("Updated {} thumb hashes", count);

    Ok(())
}

fn generate_thumb_image_hash(
    encoder: &mut ThumbHashEncoder,
    preview_path: &Path,
) -> Result<Vec<u8>, std::io::Error> {
    let thumb_image = generate_thumb_hash_raw_image(preview_path)?;
    encoder
        .encode_rgba(&thumb_image)
        .map_err(std::io::Error::other)
}
