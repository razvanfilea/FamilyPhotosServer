mod hash;
mod scan;
mod timestamp_parsing;

use axum::response::ErrorResponse;
pub use scan::scan_new_files;
use std::collections::HashSet;
use std::fs;
use std::time::Duration;
use tracing::{debug, error, info};

use crate::file_scan;
use crate::file_scan::hash::compute_photos_hash;
use crate::http::AppStateRef;

pub fn start_period_file_scanning_task(app_state: AppStateRef, scan_new_files: bool) {
    const MINUTE: u64 = 60;

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(MINUTE * 120));

        loop {
            interval.tick().await;

            if scan_new_files {
                file_scan::scan_new_files(app_state).await;
            }

            if let Err(e) = resolve_duplicates_db_entry(app_state).await {
                error!("Failed to resolve db duplicates: {:?}", e);
            }

            if let Err(e) = delete_invalid_photo_previews(app_state).await {
                error!("Failed to delete invalid photo previews: {:?}", e);
            }

            if let Err(e) = compute_photos_hash(app_state).await {
                error!("Failed to compute hashes: {}", e);
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

async fn delete_invalid_photo_previews(app_state: AppStateRef) -> Result<(), ErrorResponse> {
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
