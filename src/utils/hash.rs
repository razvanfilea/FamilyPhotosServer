use crate::http::AppStateRef;
use crate::model::photo::{Photo, PhotoBase};
use crate::utils::storage_resolver::StorageResolver;
use memmap2::Mmap;
use rayon::prelude::*;
use sha2::Digest;
use std::fs::File;
use tracing::info;

pub async fn compute_hashes(app_state: AppStateRef) -> Result<(), sqlx::Error> {
    let photos = app_state.duplicates_repo.get_photos_without_hash().await?;

    info!("Computing sha for {} photos", photos.len());
    if photos.is_empty() {
        return Ok(());
    }

    let photos_hashes: Vec<_> = photos
        .into_par_iter()
        .filter_map(|photo| compute_hash(&app_state.storage, photo))
        .collect();

    info!("Computed sha for {} photos", photos_hashes.len());

    app_state
        .duplicates_repo
        .insert_hashes(&photos_hashes)
        .await?;

    Ok(())
}

fn compute_hash(storage: &StorageResolver, photo: Photo) -> Option<(i64, String)> {
    let path = storage.resolve_photo(photo.partial_path());

    let file = File::open(path).ok()?;
    let mapped_file = unsafe { Mmap::map(&file) }.ok()?;

    let hash = sha2::Sha256::digest(&mapped_file);
    let hex_hash = base16ct::lower::encode_string(&hash);

    Some((photo.id, hex_hash))
}
