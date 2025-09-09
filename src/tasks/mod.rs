mod file_scan;
mod hash;
mod thumb_hash;
mod timestamp_parsing;
mod trash;

pub use file_scan::scan_new_files;
use std::collections::HashSet;
use std::fs;
use std::num::NonZero;
use std::time::Duration;
use tracing::{debug, error, info};

use crate::http::AppStateRef;
use crate::repo::event_log::EventLogRepo;
use crate::repo::{PhotosRepo, PhotosTransactionRepo};
pub use crate::tasks::hash::compute_photos_hash;
use crate::tasks::thumb_hash::generate_thumb_hashes;
use crate::tasks::trash::cleanup_trash;

pub fn start_periodic_tasks(
    app_state: AppStateRef,
    scan_new_files: bool,
    background_threads_count: usize,
) {
    rayon::ThreadPoolBuilder::new()
        .num_threads(if background_threads_count == 0 {
            std::thread::available_parallelism()
                .map(NonZero::get)
                .unwrap_or(0)
        } else {
            background_threads_count
        })
        .thread_name(|thread| format!("Rayon {thread}"))
        .build_global()
        .expect("Failed to build global thread pool");

    const MINUTE: u64 = 60;
    const HOUR: u64 = 60;

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(MINUTE * HOUR * 2));

        loop {
            interval.tick().await;

            if scan_new_files && let Err(e) = file_scan::scan_new_files(app_state).await {
                error!("Failed to scan new photos: {}", e);
            }

            if let Err(e) = resolve_duplicates_db_entry(app_state).await {
                error!("Failed to resolve db duplicates: {e}");
            }

            if let Err(e) = delete_invalid_photo_previews(app_state).await {
                error!("Failed to delete invalid photo previews: {e}");
            }

            if let Err(e) = compute_photos_hash(app_state).await {
                error!("Failed to compute hashes: {e}");
            }

            if let Err(e) = cleanup_trash(app_state).await {
                error!("Failed to cleanup trash: {e}");
            }

            // Thumb hash generation is based upon preview generation
            if let Err(e) = generate_thumb_hashes(app_state).await {
                error!("Failed to generate thumb hashes: {e}");
            }

            // This should ideally always remain the last
            if let Err(e) = delete_old_event_logs(app_state).await {
                error!("Failed to delete old events: {e}");
            }
        }
    });
}

async fn resolve_duplicates_db_entry(app_state: AppStateRef) -> Result<(), sqlx::Error> {
    debug!("Started resolving duplicates");

    let mut tx = app_state.pool.begin().await?;
    let photos = tx.get_photos_with_same_location().await?;

    for photo in photos {
        info!(
            "Removing duplicate DB entry with path: {}",
            photo.partial_path()
        );
        tx.delete_photo(&photo).await?;
    }

    tx.commit().await?;

    Ok(())
}

async fn delete_invalid_photo_previews(app_state: AppStateRef) -> Result<(), sqlx::Error> {
    let photos: HashSet<i64> = app_state
        .pool
        .get_all_photos()
        .await?
        .into_iter()
        .map(|p| p.id)
        .collect();

    let count = walkdir::WalkDir::new(&app_state.storage.preview_folder)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter_map(|entry| {
            let path = entry.into_path();
            path.file_stem()
                .and_then(|s| s.to_string_lossy().parse::<i64>().ok())
                .map(|id| (path, id))
        })
        .filter(|(_, photo_id)| !photos.contains(photo_id))
        .filter_map(|(path, _)| fs::remove_file(path).ok())
        .count();

    if count != 0 {
        info!("Deleted {count} invalid photo previews");
    }

    Ok(())
}

async fn delete_old_event_logs(app_state: AppStateRef) -> Result<(), sqlx::Error> {
    const MAX_EVENT_LONG_ROWS_TO_KEEP: u32 = 512;

    app_state
        .pool
        .delete_old_events(MAX_EVENT_LONG_ROWS_TO_KEEP)
        .await
}
