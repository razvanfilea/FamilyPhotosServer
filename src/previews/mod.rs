use rayon::prelude::*;
use tracing::{error, info};

pub use generate::*;

use crate::http::AppState;
use crate::model::photo::Photo;
use crate::repo::PhotosRepo;

mod generate;

pub async fn generate_all_previews(app_state: &AppState) -> sqlx::Result<()> {
    let _ = app_state.preview_generation.lock().await;
    let mut tx = app_state.pool.begin().await?;

    let missing_previews_ids = tx.get_all_photo_ids().await?.into_iter().filter(|id| {
        !app_state
            .storage
            .resolve_preview(Photo::construct_partial_preview_path(*id))
            .exists()
    });

    let mut missing_previews = Vec::with_capacity(128);
    for id in missing_previews_ids {
        let Some(photo) = tx.get_photo_without_check(id).await? else {
            continue;
        };
        missing_previews.push(photo);
    }

    info!("Generating previews for {} photos", missing_previews.len());

    let previews_generated: usize = missing_previews
        .into_par_iter()
        .map(|photo| {
            let photo_path = app_state.storage.resolve_photo(photo.partial_path());
            let preview_path = app_state
                .storage
                .resolve_preview(photo.partial_preview_path());

            if photo_path.exists()
                && !preview_path.exists()
                && let Err(e) = generate_preview(&photo_path, preview_path)
            {
                error!(
                    "Preview generation failed: {}\nCause: {e}",
                    photo_path.display()
                );
                false
            } else {
                true
            }
        })
        .map(|success| success as usize)
        .sum();

    info!("Generated previews for {} photos", previews_generated);

    Ok(())
}
