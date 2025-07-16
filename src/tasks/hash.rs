use crate::http::AppStateRef;
use crate::model::photo_hash::PhotoHash;
use rayon::prelude::*;
use sha2::Digest;
use std::fs::File;
use std::path::Path;
use tracing::{error, info};

pub async fn compute_photos_hash(app_state: AppStateRef) -> Result<(), sqlx::Error> {
    let photos = app_state
        .photo_hash_repo
        .get_photos_without_hash()
        .await?;

    if photos.is_empty() {
        return Ok(());
    }
    info!("Computing hashes for {} photos", photos.len());

    let photos_hashes: Vec<_> = photos
        .into_par_iter()
        .filter_map(|photo| {
            let path = app_state.storage.resolve_photo(photo.partial_path());

            compute_hash(&path)
                .inspect_err(|e| error!("Failed to comput hash for {}: {e}", path.display()))
                .ok()
                .map(|hash| PhotoHash { id: photo.id, hash })
        })
        .collect();

    info!("Computed hashes for {} photos", photos_hashes.len());

    for chunk in photos_hashes.chunks(1024) {
        app_state.photo_hash_repo.insert_hashes(chunk).await?;
    }

    Ok(())
}

fn compute_hash(path: &Path) -> std::io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let mapped_file = unsafe { memmap2::Mmap::map(&file)? };

    let hash = sha2::Sha256::digest(&mapped_file);
    let slice = hash.as_slice();
    let hash = slice[..slice.len() / 2].to_vec();

    Ok(hash)
}
