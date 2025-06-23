use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use tracing::{debug, error, info, warn};
use walkdir::{DirEntry, WalkDir};

use crate::User;
use crate::file_scan::timestamp_parsing;
use crate::http::AppStateRef;
use crate::model::photo::{Photo, PhotoBase, PhotoBody};

pub async fn scan_new_files(app_state: AppStateRef) {
    let instant = Instant::now();
    let users: Vec<User> = app_state
        .users_repo
        .get_users()
        .await
        .expect("Could not load users");

    debug!(
        "Started scanning user's photos: {:?}",
        users.iter().map(|user| user.id.clone()).collect::<Vec<_>>()
    );

    for user in users {
        let existing_photos: Vec<Photo> = app_state
            .photos_repo
            .get_photos_by_user(&user.id)
            .await
            .expect("Failed to get user photos");

        let user_folder_path = app_state.storage.resolve_photo(&user.id);
        let (new_photos, removed_photo_ids) =
            scan_user_photos(user, user_folder_path, existing_photos);

        if !removed_photo_ids.is_empty() {
            for chunk in removed_photo_ids.chunks(1024) {
                if let Err(e) = app_state.photos_repo.delete_photos(&chunk).await {
                    error!("Failed deleting photos: {}", e.to_string())
                }
            }
        }
        
        if !new_photos.is_empty() {
            for chunk in new_photos.chunks(1024) {
                if let Err(e) = app_state.photos_repo.insert_photos(chunk).await {
                    error!("Failed inserting photos: {}", e.to_string())
                }
            }
        }

    }
    
    info!(
        "Photos scanning completed in {} seconds",
        instant.elapsed().as_secs()
    );
}

fn scan_user_photos(
    user: User,
    user_folder_path: PathBuf,
    existing_photos: Vec<Photo>,
) -> (Vec<PhotoBody>, Vec<i64>) {
    if !user_folder_path.exists() {
        if let Err(e) = fs::create_dir(user_folder_path) {
            error!("Failed to create user's `{}` directory: {e}", user.id);
        }
        // All existing photos are considered removed if the user directory doesn't exist
        let removed_ids = existing_photos.iter().map(|p| p.id()).collect();
        return (Vec::new(), removed_ids);
    }

    let json_extension = Some(OsStr::new("json"));
    let walk_dir = WalkDir::new(user_folder_path).max_depth(2);

    let disk_entries_with_name: HashMap<String, DirEntry> = walk_dir
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|entry| {
            let path = entry.path();
            !path.is_dir() && path.extension() != json_extension
        })
        .map(|entry| {
            let filename = entry.file_name().to_string_lossy().to_string();
            let folder = get_folder_name(&entry);
            (
                PhotoBody::construct_full_name(&filename, folder.as_deref()),
                entry,
            )
        })
        .collect();

    // Find removed photos (exist in DB but not on disk)
    let removed_photo_ids: Vec<i64> = existing_photos
        .iter()
        .filter(|photo| !disk_entries_with_name.contains_key(&photo.full_name()))
        .map(|photo| photo.id())
        .collect();

    // Find new photos (exist on disk but not in DB)
    let existing_photos_names: HashSet<String> = existing_photos
        .iter()
        .map(|photo| photo.full_name())
        .collect();

    let new_photos: Vec<PhotoBody> = disk_entries_with_name
        .into_par_iter()
        .filter(|(full_name, _)| !existing_photos_names.contains(full_name))
        .filter_map(|(_, entry)| parse_image(user.id.clone(), entry))
        .collect();

    info!(
        "User {}: found {} new photos, {} removed photos",
        user.id,
        new_photos.len(),
        removed_photo_ids.len()
    );

    (new_photos, removed_photo_ids)
}

pub fn parse_image(user_name: String, entry: DirEntry) -> Option<PhotoBody> {
    let path = entry.path();

    if let Some(timestamp) = timestamp_parsing::get_timestamp_for_path(path) {
        let file_size = entry.metadata().map_or(0i64, |data| data.len() as i64);
        let folder = get_folder_name(&entry);

        Some(PhotoBody::new(
            user_name,
            entry.file_name().to_string_lossy().to_string(),
            timestamp,
            file_size,
            folder,
        ))
    } else {
        warn!("No timestamp: {}", path.display());
        None
    }
}

fn get_folder_name(entry: &DirEntry) -> Option<String> {
    if entry.depth() == 2 {
        entry
            .path()
            .parent()
            .and_then(|p| p.file_name())
            .map(|f| f.to_string_lossy().to_string())
    } else {
        None
    }
}
