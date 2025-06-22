use std::ffi::OsStr;
use std::fs;
use std::time::Instant;

use rayon::prelude::*;
use tokio::task;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};
use walkdir::{DirEntry, WalkDir};

use crate::file_scan::timestamp;
use crate::http::AppStateRef;
use crate::model::photo::{Photo, PhotoBase, PhotoBody};
use crate::{AppState, StorageResolver, User};

pub struct DataScan {
    results: Vec<(User, Vec<PhotoBody>)>,
}

impl DataScan {
    pub fn run(app_state: AppStateRef) -> JoinHandle<()> {
        task::spawn(async move {
            let users: Vec<User> = app_state
                .users_repo
                .get_users()
                .await
                .expect("Could not load users");

            let instant = Instant::now();
            let data_scan = Self::scan(users, &app_state.storage);
            data_scan.update_database(&app_state).await;

            debug!(
                "Photos scanning completed in {} seconds",
                instant.elapsed().as_secs()
            );
        })
    }

    fn scan(users: Vec<User>, storage: &StorageResolver) -> Self {
        debug!(
            "Started scanning user's photos: {:?}",
            users.iter().map(|user| user.id.clone()).collect::<Vec<_>>()
        );

        let results = users
            .into_par_iter()
            .map(|user| Self::scan_user_photos(storage, user))
            .collect::<Vec<_>>();

        Self { results }
    }

    fn scan_user_photos(storage: &StorageResolver, user: User) -> (User, Vec<PhotoBody>) {
        let mut photos = Vec::with_capacity(8192 * 4);

        let user_path = storage.resolve_photo(&user.id);
        if !user_path.exists() {
            fs::create_dir(user_path).unwrap()
        } else {
            let json_extension = Some(OsStr::new("json"));
            let walk_dir = WalkDir::new(user_path).max_depth(2);

            for entry in walk_dir.into_iter().filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_dir() || path.extension() == json_extension {
                    continue;
                }

                if let Some(photo) = Self::parse_image(user.id.clone(), entry) {
                    photos.push(photo)
                }
            }
        }

        info!("Finished scanning for {}", user.id);

        (user, photos)
    }

    pub fn parse_image(user_name: String, entry: DirEntry) -> Option<PhotoBody> {
        let path = entry.path();

        if let Some(timestamp) = timestamp::get_timestamp_for_path(path) {
            Some(PhotoBody::new(
                user_name,
                entry.file_name().to_string_lossy().to_string(),
                timestamp,
                fs::metadata(path).map_or(0i64, |data| data.len() as i64),
                if entry.depth() == 2 {
                    Some(path.parent()?.file_name()?.to_string_lossy().to_string())
                } else {
                    None
                },
            ))
        } else {
            warn!("No timestamp: {}", path.display());
            None
        }
    }

    async fn update_database(self, app_state: &AppState) {
        let storage = &app_state.storage;
        let photos_repo = &app_state.photos_repo;

        for (user, mut found_photos) in self.results {
            let existing_photos: Vec<Photo> = photos_repo
                .get_photos_by_user(&user.id)
                .await
                .expect("Failed to get user photos");
            let existing_photos_names: Vec<String> = existing_photos
                .iter()
                .map(|photo| photo.full_name())
                .collect();

            info!("Scanned {} photos in user {}", found_photos.len(), user.id);

            // Add any photo that was not already in the db
            // Keep only new photos
            found_photos.retain(|photo| !existing_photos_names.contains(&photo.full_name()));

            if !found_photos.is_empty() {
                info!(
                    "Adding {} new photos to user {}",
                    found_photos.len(),
                    user.id
                );

                for chunk in found_photos.chunks(512) {
                    if let Err(e) = photos_repo.insert_photos(chunk).await {
                        error!("Failed inserting photos: {}", e.to_string())
                    }
                }
            }

            let removed_photos = existing_photos
                .iter()
                .filter(|photo| !storage.resolve_photo(photo.partial_path()).exists())
                .map(|photo| photo.id())
                .collect::<Vec<i64>>();

            if !removed_photos.is_empty() {
                info!(
                    "Removing {} photos from user {}",
                    removed_photos.len(),
                    user.id
                );

                if let Err(e) = app_state.photos_repo.delete_photos(&removed_photos).await {
                    error!("Failed deleting photos: {}", e.to_string())
                }
            }
        }
    }
}
