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
pub use timestamp_parsing::get_timestamp_for_path;
use tracing::{debug, error, info};

use crate::http::AppStateRef;
use crate::previews::generate_all_previews;
use crate::repo::{
    EventLogRepo, FolderEventLogRepo, FoldersRepo, PhotosRepo, PhotosTransactionRepo,
};
pub use crate::tasks::hash::compute_photos_hash;
pub use crate::tasks::thumb_hash::generate_thumb_hashes;
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

            if let Err(e) = compute_photos_hash(app_state).await {
                error!("Failed to compute hashes: {e}");
            }

            if let Err(e) = cleanup_trash(app_state).await {
                error!("Failed to cleanup trash: {e}");
            }

            if let Err(e) = delete_invalid_photo_previews(app_state).await {
                error!("Failed to delete invalid photo previews: {e}");
            }

            if let Err(e) = generate_all_previews(app_state).await {
                error!("Failed to generate all previews: {e}");
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

    let mut tx = app_state.write_pool.begin().await?;
    let folder_map = tx.as_mut().get_folder_name_map().await?;
    let photos = tx.get_photos_with_same_location().await?;

    for photo in photos {
        let folder_name = photo.folder_id.and_then(|id| folder_map.get(&id).cloned());
        info!(
            "Removing duplicate DB entry with path: {}",
            photo.partial_path(folder_name.as_deref())
        );
        tx.delete_photo(&photo).await?;
    }

    tx.commit().await?;

    Ok(())
}

async fn delete_invalid_photo_previews(app_state: AppStateRef) -> Result<(), sqlx::Error> {
    use crate::previews::MIN_PREVIEW_SIZE;

    let photos: HashSet<i64> = app_state
        .read_pool
        .get_all_photo_ids()
        .await?
        .into_iter()
        .collect();

    let preview_folder = app_state.storage.preview_folder.clone();
    let (orphan_count, invalid_count) = tokio::task::spawn_blocking(move || {
        let mut orphan_count = 0;
        let mut invalid_count = 0;

        for entry in walkdir::WalkDir::new(preview_folder)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            if let Ok(metadata) = fs::metadata(path)
                && metadata.len() < MIN_PREVIEW_SIZE
            {
                if fs::remove_file(path).is_ok() {
                    invalid_count += 1;
                }
                continue;
            }

            if let Some(stem) = path.file_stem()
                && let Ok(photo_id) = stem.to_string_lossy().parse::<i64>()
                && !photos.contains(&photo_id)
                && fs::remove_file(path).is_ok()
            {
                orphan_count += 1;
            }
        }

        (orphan_count, invalid_count)
    })
    .await
    .map_err(|e| sqlx::Error::Io(std::io::Error::other(e)))?;

    if orphan_count > 0 || invalid_count > 0 {
        info!(
            "Deleted {} orphan and {} invalid photo previews",
            orphan_count, invalid_count
        );
    }

    Ok(())
}

async fn delete_old_event_logs(app_state: AppStateRef) -> Result<(), sqlx::Error> {
    const MAX_EVENT_LOG_ROWS_TO_KEEP: u32 = 512;
    const MAX_FOLDER_EVENT_ROWS_PER_FOLDER: u32 = 500;

    app_state
        .write_pool
        .delete_old_events(MAX_EVENT_LOG_ROWS_TO_KEEP)
        .await?;

    app_state
        .write_pool
        .delete_old_folder_events(MAX_FOLDER_EVENT_ROWS_PER_FOLDER)
        .await
}
