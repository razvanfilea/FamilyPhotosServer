use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::{OnceCell, Semaphore};
use tokio::task::JoinSet;
use tracing::{error, info, warn};

pub use generate::*;

use crate::http::AppState;
use crate::model::photo::Photo;
use crate::repo::{FoldersRepo, MAX_BOUND_IDS, PhotosRepo};
use crate::utils::storage_resolver::StorageResolver;

mod generate;

/// Returns true if preview exists and has valid size
pub fn is_valid_preview(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.len() >= MIN_PREVIEW_SIZE)
        .unwrap_or(false)
}

/// A generated, cached version of a photo
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rendition {
    /// 320px thumbnail used by grids: `PREVIEWS_PATH/<id>.webp`
    Preview,
    /// ~2048px image used by the viewer: `PREVIEWS_PATH/screen/<id>.webp`
    Screen,
}

pub const SCREEN_RENDITION_FOLDER: &str = "screen";

impl Rendition {
    pub fn partial_path(self, photo_id: i64) -> String {
        match self {
            Rendition::Preview => Photo::construct_partial_preview_path(photo_id),
            Rendition::Screen => format!(
                "{SCREEN_RENDITION_FOLDER}/{}",
                Photo::construct_partial_preview_path(photo_id)
            ),
        }
    }

    fn generate(self, load_path: &Path, save_path: &Path) -> io::Result<()> {
        match self {
            Rendition::Preview => generate_preview(load_path, save_path),
            Rendition::Screen => generate_screen_rendition(load_path, save_path),
        }
    }
}

type GenerationResult = Result<(), Arc<io::Error>>;
type InFlightGenerations = HashMap<(Rendition, i64), Arc<OnceCell<GenerationResult>>>;

/// Bounds the number of concurrent `magick`/`ffmpeg` processes and makes concurrent requests for
/// the same rendition wait for a single generation.
pub struct PreviewGenerator {
    semaphore: Semaphore,
    permits: usize,
    in_flight: Mutex<InFlightGenerations>,
}

impl Default for PreviewGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl PreviewGenerator {
    pub fn new() -> Self {
        let permits = std::thread::available_parallelism()
            .map(|p| p.get() / 2)
            .unwrap_or(0)
            .max(2);

        Self {
            semaphore: Semaphore::new(permits),
            permits,
            in_flight: Mutex::new(HashMap::new()),
        }
    }

    /// Generates the rendition of `photo_id` from `source` into `target`, unless a valid one
    /// already exists.
    pub async fn ensure(
        &self,
        rendition: Rendition,
        photo_id: i64,
        source: &Path,
        target: &Path,
    ) -> io::Result<()> {
        if tokio::fs::metadata(target)
            .await
            .is_ok_and(|m| m.len() >= MIN_PREVIEW_SIZE)
        {
            return Ok(());
        }

        let key = (rendition, photo_id);
        let cell = self
            .in_flight
            .lock()
            .expect("Preview generation map poisoned")
            .entry(key)
            .or_default()
            .clone();

        let result = cell
            .get_or_init(|| async {
                let _permit = self
                    .semaphore
                    .acquire()
                    .await
                    .expect("Preview semaphore is never closed");

                let source = source.to_path_buf();
                let target = target.to_path_buf();
                tokio::task::spawn_blocking(move || {
                    if is_valid_preview(&target) {
                        return Ok(());
                    }
                    rendition.generate(&source, &target)
                })
                .await
                .unwrap_or_else(|e| Err(io::Error::other(e)))
                .map_err(Arc::new)
            })
            .await
            .clone();

        // Later requests either find the file or, after a failure, try again
        {
            let mut in_flight = self
                .in_flight
                .lock()
                .expect("Preview generation map poisoned");
            if in_flight
                .get(&key)
                .is_some_and(|existing| Arc::ptr_eq(existing, &cell))
            {
                in_flight.remove(&key);
            }
        }

        result.map_err(|e| io::Error::new(e.kind(), e.to_string()))
    }
}

/// Removes every cached rendition of a photo (missing files are ignored)
pub async fn remove_renditions(storage: &StorageResolver, photo_id: i64) {
    for rendition in [Rendition::Preview, Rendition::Screen] {
        let path = storage.resolve_preview(rendition.partial_path(photo_id));
        if let Err(e) = tokio::fs::remove_file(&path).await
            && e.kind() != io::ErrorKind::NotFound
        {
            warn!("Failed to remove {}: {e}", path.display());
        }
    }
}

pub async fn generate_all_previews(app_state: &'static AppState) -> sqlx::Result<()> {
    let all_ids = app_state.read_pool.get_all_photo_ids().await?;
    let missing_ids = tokio::task::spawn_blocking(move || {
        all_ids
            .into_iter()
            .filter(|id| {
                let preview_path = app_state
                    .storage
                    .resolve_preview(Photo::construct_partial_preview_path(*id));
                !is_valid_preview(&preview_path)
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| sqlx::Error::Io(std::io::Error::other(e)))?;

    let mut missing_previews = Vec::with_capacity(missing_ids.len());
    for chunk in missing_ids.chunks(MAX_BOUND_IDS) {
        missing_previews.extend(app_state.read_pool.get_photos_by_ids(chunk).await?);
    }

    let folder_map = app_state.read_pool.get_folder_name_map().await?;

    info!("Generating previews for {} photos", missing_previews.len());

    // Leave one permit free so that previews requested by users don't wait for the whole batch
    let max_concurrent = app_state.previews.permits.saturating_sub(1).max(1);
    let mut tasks = JoinSet::new();
    let mut previews_generated = 0usize;

    for photo in missing_previews {
        if tasks.len() >= max_concurrent
            && let Some(result) = tasks.join_next().await
        {
            previews_generated += result.unwrap_or(false) as usize;
        }

        let folder_name = photo
            .folder_id
            .and_then(|id| folder_map.get(&id).map(|s| s.as_str()));
        let photo_path = app_state
            .storage
            .resolve_photo(photo.partial_path(folder_name));
        let preview_path = app_state
            .storage
            .resolve_preview(photo.partial_preview_path());

        tasks.spawn(async move {
            if !tokio::fs::try_exists(&photo_path).await.unwrap_or(false) {
                return false;
            }
            match app_state
                .previews
                .ensure(Rendition::Preview, photo.id, &photo_path, &preview_path)
                .await
            {
                Ok(()) => true,
                Err(e) => {
                    error!(
                        "Preview generation failed: {}\nCause: {e}",
                        photo_path.display()
                    );
                    false
                }
            }
        });
    }

    while let Some(result) = tasks.join_next().await {
        previews_generated += result.unwrap_or(false) as usize;
    }

    info!("Generated previews for {} photos", previews_generated);

    Ok(())
}

/// Path helper used by handlers: the absolute path of a rendition
pub fn rendition_path(storage: &StorageResolver, rendition: Rendition, photo_id: i64) -> PathBuf {
    storage.resolve_preview(rendition.partial_path(photo_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rendition_partial_path() {
        assert_eq!(Rendition::Preview.partial_path(12), "12.webp");
        assert_eq!(Rendition::Screen.partial_path(12), "screen/12.webp");
    }

    #[tokio::test]
    async fn test_ensure_existing_and_failures() {
        let dir = tempfile::tempdir().unwrap();
        let generator = PreviewGenerator::new();
        assert!(generator.permits >= 2);

        // A valid rendition is used as it is, even if the source doesn't exist
        let existing = dir.path().join("1.webp");
        std::fs::write(&existing, [1u8; 200]).unwrap();
        let missing_source = dir.path().join("missing.jpg");
        generator
            .ensure(Rendition::Preview, 1, &missing_source, &existing)
            .await
            .unwrap();

        // Concurrent failing requests all fail, and nothing stays in flight so that a later
        // request retries
        let target = dir.path().join("screen").join("2.webp");
        let (a, b) = tokio::join!(
            generator.ensure(Rendition::Screen, 2, &missing_source, &target),
            generator.ensure(Rendition::Screen, 2, &missing_source, &target),
        );
        assert!(a.is_err() && b.is_err());
        assert!(!target.exists());
        assert!(generator.in_flight.lock().unwrap().is_empty());
    }
}
