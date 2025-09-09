use crate::http::AppStateRef;
use crate::model::photo_hash::PhotoHash;
use crate::repo::PhotosHashRepo;
use crate::utils::crop_blake_3_hash;
use rayon::prelude::*;
use std::path::Path;
use tokio::task::spawn_blocking;
use tracing::{error, info};

pub async fn compute_photos_hash(app_state: AppStateRef) -> Result<(), sqlx::Error> {
    const CHUNK_SIZE: usize = 256;

    let mut tx = app_state.pool.begin().await?;
    let photos = tx.get_photos_without_hash().await?;

    if photos.is_empty() {
        return Ok(());
    }
    info!("Computing hashes for {} photos", photos.len());

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

    spawn_blocking(move || {
        photos.par_chunks(CHUNK_SIZE).for_each(|chunk| {
            let chunk: Vec<_> = chunk
                .iter()
                .filter_map(|photo| {
                    let path = app_state.storage.resolve_photo(photo.partial_path());

                    compute_hash(&path)
                        .inspect_err(|e| {
                            error!("Failed to compute hash for {}: {e}", path.display())
                        })
                        .ok()
                        .map(|hash| PhotoHash { id: photo.id, hash })
                })
                .collect();

            if let Err(e) = sender.send(chunk) {
                error!("Failed to send hashes over channel: {e}");
            }
        });

        drop(sender);
    });

    let mut hashes_count = 0;

    while let Some(chunk) = receiver.recv().await {
        tx.insert_hashes(&chunk).await?;
        hashes_count += chunk.len();
    }

    tx.commit().await?;

    info!("Computed hashes for {hashes_count} photos");

    Ok(())
}

fn compute_hash(path: &Path) -> std::io::Result<Vec<u8>> {
    let hash = blake3::Hasher::new().update_mmap(path)?.finalize();
    let hash = crop_blake_3_hash(hash.as_bytes());

    Ok(hash)
}
