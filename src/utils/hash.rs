use std::fs::File;
use memmap2::Mmap;
use rayon::prelude::*;
use sha2::Digest;
use sqlx::{query_as, SqlitePool};
use tracing::info;
use crate::model::photo::{Photo, PhotoBase};
use crate::utils::storage_resolver::StorageResolver;

pub async fn compute_hashes(pool: SqlitePool, storage: StorageResolver) -> Result<(), sqlx::Error> {
    let photos = query_as!(
        Photo,
        "select p.* from photos p left join photos_extras e on p.id = e.id where e.sha is null"
    )
        .fetch_all(&pool)
        .await?;

    info!("Computing sha for {} photos", photos.len());
    if photos.is_empty() {
        return Ok(());
    }

    let photos_hashes: Vec<_> = photos
        .into_par_iter()
        .filter_map(|photo| compute_hash(&storage, photo))
        .collect();

    info!("Computed sha for {} photos", photos_hashes.len());

    for (id, sha) in photos_hashes {
        sqlx::query!(
            "insert into photos_extras (id, sha) VALUES ($1, $2)",
            id,
            sha
        )
            .execute(&pool)
            .await?;
    }

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

