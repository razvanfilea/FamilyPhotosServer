mod file_scan;
mod hash;
mod timestamp_parsing;

pub use file_scan::scan_new_files;
use std::collections::HashSet;
use std::fs;
use std::time::Duration;
use tracing::{debug, error, info};

use crate::http::AppStateRef;
pub use crate::tasks::hash::compute_photos_hash;

pub fn start_period_tasks(app_state: AppStateRef, scan_new_files: bool) {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60;

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(MINUTE * HOUR * 2));

        loop {
            interval.tick().await;

            if scan_new_files {
                file_scan::scan_new_files(app_state).await;
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

            if let Err(e) = delete_old_event_logs(app_state).await {
                error!("Failed to delete old events: {e}");
            }
        }
    });
}
async fn resolve_duplicates_db_entry(app_state: AppStateRef) -> Result<(), sqlx::Error> {
    debug!("Started resolving duplicates");

    let photos = app_state
        .photos_repo
        .get_photos_with_same_location()
        .await?;

    for photo in photos {
        info!(
            "Removing duplicate DB entry with path: {}",
            photo.partial_path()
        );
        app_state.photos_repo.delete_photo(&photo).await?;
    }

    Ok(())
}

async fn delete_invalid_photo_previews(app_state: AppStateRef) -> Result<(), sqlx::Error> {
    let photos: HashSet<i64> = app_state
        .photos_repo
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
        .event_log_repo
        .delete_old_events(MAX_EVENT_LONG_ROWS_TO_KEEP)
        .await
}
