use rayon::prelude::*;
use tracing::error;

pub use generate::*;

use crate::http::AppState;
use crate::model::photo::PhotoBase;

mod generate;

pub async fn generate_all_previews(app_state: &AppState) -> Result<(), String> {
    app_state
        .photos_repo
        .get_all_photos()
        .await
        .map_err(|_| "Could not load photos".to_string())?
        .into_par_iter()
        .for_each(|photo| {
            let photo_path = app_state.storage.resolve_photo(photo.partial_path());
            let preview_path = app_state
                .storage
                .resolve_preview(photo.partial_preview_path());

            if photo_path.exists() && !preview_path.exists() {
                match generate_preview(&photo_path, preview_path) {
                    Ok(_) => {}
                    Err(e) => {
                        error!(
                            "Preview generation failed for video: {}\nCause: {e}",
                            photo_path.display()
                        )
                    }
                }
            }
        });

    Ok(())
}
