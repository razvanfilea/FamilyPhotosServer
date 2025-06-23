mod exif;
mod extras;
mod scan;
mod timestamp_parsing;

use axum::response::ErrorResponse;
pub use scan::scan_new_files;
use std::time::Duration;
use tracing::{debug, error, info};

use crate::file_scan;
use crate::file_scan::extras::compute_photos_extras;
use crate::http::AppStateRef;
use crate::model::photo::PhotoBase;

pub fn start_period_file_scanning_task(app_state: AppStateRef, scan_new_files: bool) {
    const MINUTE: u64 = 60;

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(MINUTE * 120));

        loop {
            interval.tick().await;

            if let Err(e) = resolve_duplicates_db_entry(app_state).await {
                error!("Failed to resolve db duplicates: {:?}", e);
            }

            if let Err(e) = compute_photos_extras(app_state).await {
                error!("Failed to compute hashes: {}", e);
            }

            if scan_new_files {
                file_scan::scan_new_files(app_state).await;
            }
        }
    });
}
async fn resolve_duplicates_db_entry(app_state: AppStateRef) -> Result<(), ErrorResponse> {
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
        app_state.photos_repo.delete_photo(photo.id).await?;
    }

    Ok(())
}
