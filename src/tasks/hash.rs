use crate::http::AppStateRef;
use crate::model::photo_hash::PhotoHash;
use crate::repo::{FoldersRepo, PhotosHashRepo};
use crate::utils::crop_blake_3_hash;
use rayon::prelude::*;
use std::path::Path;
use tokio::task::spawn_blocking;
use tracing::{error, info};

const BATCH_SIZE: i64 = 256;

pub async fn compute_photos_hash(app_state: AppStateRef) -> Result<(), sqlx::Error> {
    compute_photos_hash_in_batches(app_state, BATCH_SIZE).await
}

/// Hashes photos in batches, so no database connection is held while reading files.
/// Photos are paginated by id, so a photo that can't be hashed is skipped, not retried.
async fn compute_photos_hash_in_batches(
    app_state: AppStateRef,
    batch_size: i64,
) -> Result<(), sqlx::Error> {
    let mut last_id = 0;
    let mut hashes_count = 0;

    loop {
        let photos = app_state
            .read_pool
            .get_photos_without_hash(last_id, batch_size)
            .await?;

        let Some(last_photo) = photos.last() else {
            break;
        };
        last_id = last_photo.id;

        let folder_map = app_state.read_pool.get_folder_name_map().await?;

        let hashes: Vec<PhotoHash> = spawn_blocking(move || {
            photos
                .par_iter()
                .filter_map(|photo| {
                    let folder_name = photo
                        .folder_id
                        .and_then(|id| folder_map.get(&id).map(|s| s.as_str()));
                    let path = app_state
                        .storage
                        .resolve_photo(photo.partial_path(folder_name));

                    compute_hash(&path)
                        .inspect_err(|e| {
                            error!("Failed to compute hash for {}: {e}", path.display())
                        })
                        .ok()
                        .map(|hash| PhotoHash { id: photo.id, hash })
                })
                .collect()
        })
        .await
        .unwrap_or_else(|e| {
            error!("Hashing batch panicked: {e}");
            Vec::new()
        });

        hashes_count += save_hashes(app_state, hashes).await;
    }

    if hashes_count > 0 {
        info!("Computed hashes for {hashes_count} photos");
    }

    Ok(())
}

/// Returns how many hashes were saved.
async fn save_hashes(app_state: AppStateRef, mut hashes: Vec<PhotoHash>) -> usize {
    let Err(e) = app_state.write_pool.insert_hashes(&hashes).await else {
        return hashes.len();
    };
    error!("Failed to save a batch of {} hashes: {e}", hashes.len());

    // Most likely a photo was deleted since it was read: retry without the missing ones.
    let ids: Vec<i64> = hashes.iter().map(|h| h.id).collect();
    match app_state.read_pool.get_existing_photo_ids(&ids).await {
        Ok(existing) if existing.len() < ids.len() => {
            hashes.retain(|h| existing.contains(&h.id));
        }
        _ => return 0,
    }

    match app_state.write_pool.insert_hashes(&hashes).await {
        Ok(()) => hashes.len(),
        Err(e) => {
            error!("Failed to save the batch of hashes again: {e}");
            0
        }
    }
}

fn compute_hash(path: &Path) -> std::io::Result<Vec<u8>> {
    let hash = blake3::Hasher::new().update_mmap(path)?.finalize();
    let hash = crop_blake_3_hash(hash.as_bytes());

    Ok(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::http::AppState;
    use crate::model::user::PUBLIC_USER_FOLDER;
    use crate::repo::PhotosTransactionRepo;
    use crate::repo::tests::create_test_photo;
    use crate::utils::storage_resolver::StorageResolver;
    use sqlx::SqlitePool;
    use sqlx::migrate::MigrateDatabase;

    /// Inserts `count` public photos named `<n>.jpg`, writing a file for each one not in `missing`.
    async fn setup(
        read_pool: &SqlitePool,
        write_pool: &SqlitePool,
        dir: &Path,
        count: usize,
        missing: &[usize],
    ) -> AppStateRef {
        let storage = StorageResolver::new(dir.join("storage"), dir.join("previews"));
        std::fs::create_dir_all(storage.storage_folder.join(PUBLIC_USER_FOLDER)).unwrap();

        let photos: Vec<_> = (0..count)
            .map(|i| create_test_photo(0, None, None, &format!("{i}.jpg")))
            .collect();
        for i in (0..count).filter(|i| !missing.contains(i)) {
            let path = storage.resolve_photo(format!("{PUBLIC_USER_FOLDER}/{i}.jpg"));
            std::fs::write(path, format!("content {i}")).unwrap();
        }
        let mut tx = write_pool.begin().await.unwrap();
        tx.insert_photos(&photos).await.unwrap();
        tx.commit().await.unwrap();

        Box::leak(Box::new(AppState::new(
            read_pool.clone(),
            write_pool.clone(),
            storage,
        )))
    }

    async fn hashes_count(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("select count(*) from photos_hash")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[sqlx::test]
    async fn test_missing_file_does_not_stop_later_batches(pool: SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let state = setup(&pool, &pool, dir.path(), 10, &[1, 2]).await;

        compute_photos_hash_in_batches(state, 3).await.unwrap();

        assert_eq!(hashes_count(&pool).await, 8);
    }

    #[sqlx::test]
    async fn test_hashes_of_deleted_photos_are_skipped_on_save(pool: SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let state = setup(&pool, &pool, dir.path(), 3, &[]).await;
        let ids: Vec<i64> = sqlx::query_scalar("select id from photos order by id")
            .fetch_all(&pool)
            .await
            .unwrap();
        let hash = |id| PhotoHash {
            id,
            hash: vec![1, 2, 3],
        };

        let saved = save_hashes(state, vec![hash(ids[0]), hash(9999), hash(ids[1])]).await;

        assert_eq!(saved, 2);
        assert_eq!(hashes_count(&pool).await, 2);
    }

    #[tokio::test]
    async fn test_hashes_are_committed_while_hashing_is_running() {
        let dir = tempfile::tempdir().unwrap();
        let db_url = format!("sqlite:{}", dir.path().join("test.db").display());
        sqlx::Sqlite::create_database(&db_url).await.unwrap();
        let (read_pool, write_pool) = db::init_pools(&db_url).await;
        db::run_migrations(&write_pool).await;
        let state = setup(&read_pool, &write_pool, dir.path(), 300, &[]).await;

        let task = tokio::spawn(compute_photos_hash_in_batches(state, 2));

        // A single long transaction would show either no hashes or all of them
        let mut saw_partial = false;
        while !task.is_finished() {
            sqlx::query("update photos set file_size = file_size + 1")
                .execute(&write_pool)
                .await
                .unwrap();
            let count = hashes_count(&read_pool).await;
            saw_partial |= count > 0 && count < 300;
        }
        task.await.unwrap().unwrap();
        assert!(saw_partial);
    }
}
