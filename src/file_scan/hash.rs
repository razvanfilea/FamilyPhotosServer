use crate::http::AppStateRef;
use crate::model::photo::PhotoBase;
use memmap2::Mmap;
use rayon::prelude::*;
use sha2::Digest;
use std::fs::File;
use std::path::Path;
use tracing::info;

pub async fn compute_hashes(app_state: AppStateRef) -> Result<(), sqlx::Error> {
    let photos = app_state.duplicates_repo.get_photos_without_hash().await?;

    if photos.is_empty() {
        return Ok(());
    }
    info!("Computing sha for {} photos", photos.len());

    let photos_hashes: Vec<_> = photos
        .into_par_iter()
        .filter_map(|photo| {
            let path = app_state.storage.resolve_photo(photo.partial_path());
            compute_file_hash(&path).ok().map(|hash| (photo.id, hash))
        })
        .collect();

    info!("Computed sha for {} photos", photos_hashes.len());

    for chunk in photos_hashes.chunks(1024) {
        app_state.duplicates_repo.insert_hashes(chunk).await?;
    }

    Ok(())
}

fn compute_file_hash(file_path: &Path) -> std::io::Result<String> {
    let file = File::open(file_path)?;
    let mapped_file = unsafe { Mmap::map(&file) }?;

    let hash = sha2::Sha256::digest(&mapped_file);
    let hex_hash = base16ct::lower::encode_string(&hash);

    Ok(hex_hash)
}
