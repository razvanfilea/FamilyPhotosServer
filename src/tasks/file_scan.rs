use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use tracing::{error, info, warn};
use walkdir::{DirEntry, WalkDir};

use crate::http::AppStateRef;
use crate::model::photo::Photo;
use crate::model::user::PUBLIC_USER_FOLDER;
use crate::previews;
use crate::repo::{FoldersRepo, MAX_BOUND_IDS, PhotosRepo, PhotosTransactionRepo};
use crate::tasks::timestamp_parsing;

/// What a walk of one user's directory found, to be applied by [`apply_user_scan`]
struct UserScan {
    /// Files without a row, with the name of the folder they are in
    new_photos: Vec<(Photo, Option<String>)>,
    /// Rows (as they were read) without a file
    removed_photos: Vec<Photo>,
}

/// Brings the database in line with the files: new files get rows, rows of deleted files are
/// removed. The disk is read without holding any database connection; the write connection is
/// only taken, per user and briefly, when there is something to change.
pub async fn scan_new_files(app_state: AppStateRef) -> sqlx::Result<()> {
    let instant = Instant::now();
    let mut users: Vec<_> = app_state
        .users_repo
        .get_users()
        .await?
        .into_iter()
        .map(|user| Some(user.id))
        .collect();

    users.push(None); // Scan the public folder

    for user_id in users {
        let scan = scan_user_files(app_state, user_id.as_deref()).await?;
        if !scan.new_photos.is_empty() || !scan.removed_photos.is_empty() {
            apply_user_scan(app_state, user_id.as_deref(), scan).await?;
        }
    }

    info!(
        "Photos scanning completed in {} seconds",
        instant.elapsed().as_secs()
    );

    Ok(())
}

async fn scan_user_files(app_state: AppStateRef, user_id: Option<&str>) -> sqlx::Result<UserScan> {
    let existing_photos = app_state.read_pool.get_photos_by_user(user_id).await?;
    let folder_map = app_state.read_pool.get_folder_name_map().await?;
    let user_folder_path = app_state.storage.resolve_folder(user_id, None);

    let owner = user_id.map(ToOwned::to_owned);
    tokio::task::spawn_blocking(move || {
        scan_user_photos(
            owner.as_deref(),
            user_folder_path,
            existing_photos,
            &folder_map,
        )
    })
    .await
    .map_err(|e| sqlx::Error::Io(std::io::Error::other(e)))
}

/// Writes a scan in one transaction. The scan is stale by now, so every new file and every
/// removal is checked against the rows as they are in the transaction: an upload or another scan
/// that got there first must not be duplicated or deleted.
async fn apply_user_scan(
    app_state: AppStateRef,
    user_id: Option<&str>,
    scan: UserScan,
) -> sqlx::Result<()> {
    let mut tx = app_state.write_pool.begin().await?;

    let current_photos = tx.get_photos_by_user(user_id).await?;
    let folder_map = tx.as_mut().get_folder_name_map().await?;
    let folder_name =
        |folder_id: Option<i64>| folder_id.and_then(|id| folder_map.get(&id).map(String::as_str));

    let current_paths: HashSet<String> = current_photos
        .iter()
        .map(|photo| photo.full_name(folder_name(photo.folder_id)))
        .collect();
    let current_by_id: HashMap<i64, &Photo> = current_photos
        .iter()
        .map(|photo| (photo.id, photo))
        .collect();

    let mut removed_photo_ids = Vec::new();
    for photo in &scan.removed_photos {
        let unchanged = current_by_id.get(&photo.id).is_some_and(|current| {
            current.name == photo.name && current.folder_id == photo.folder_id
        });
        let path = app_state
            .storage
            .resolve_photo(photo.partial_path(folder_name(photo.folder_id)));
        if unchanged && !tokio::fs::try_exists(path).await.unwrap_or(false) {
            removed_photo_ids.push(photo.id);
        }
    }

    for chunk in removed_photo_ids.chunks(MAX_BOUND_IDS) {
        if let Err(e) = tx.delete_photos(chunk).await {
            error!("Failed deleting photos: {e}")
        }
    }
    for photo_id in &removed_photo_ids {
        previews::remove_renditions(&app_state.storage, *photo_id).await;
    }

    let new_photos: Vec<_> = scan
        .new_photos
        .into_iter()
        .filter(|(photo, folder_name)| {
            !current_paths.contains(&Photo::construct_full_name(
                &photo.name,
                folder_name.as_deref(),
            ))
        })
        .collect();

    let mut name_to_id: HashMap<String, i64> = HashMap::new();
    for (_, folder_name) in &new_photos {
        if let Some(name) = folder_name.as_deref()
            && !name_to_id.contains_key(name)
            && let Some(id) = tx.as_mut().upsert_folder(user_id, Some(name)).await?
        {
            name_to_id.insert(name.to_string(), id);
        }
    }

    let new_photos: Vec<Photo> = new_photos
        .into_iter()
        .map(|(mut photo, folder_name)| {
            photo.folder_id = folder_name
                .as_deref()
                .and_then(|n| name_to_id.get(n).copied());
            photo
        })
        .collect();

    for chunk in new_photos.chunks(MAX_BOUND_IDS) {
        if let Err(e) = tx.insert_photos(chunk).await {
            error!("Failed inserting photos: {e}")
        }
    }

    tx.commit().await
}

fn scan_user_photos(
    user_id: Option<&str>,
    user_folder_path: PathBuf,
    existing_photos: Vec<Photo>,
    folder_map: &HashMap<i64, String>,
) -> UserScan {
    if !user_folder_path.exists() {
        if let Err(e) = fs::create_dir(user_folder_path) {
            error!(
                "Failed to create user's `{}` directory: {e}",
                user_id.unwrap_or(PUBLIC_USER_FOLDER)
            );
        }
        return UserScan {
            new_photos: Vec::new(),
            removed_photos: existing_photos,
        };
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
                Photo::construct_full_name(&filename, folder.as_deref()),
                entry,
            )
        })
        .collect();

    let folder_name = |photo: &Photo| {
        photo
            .folder_id
            .and_then(|id| folder_map.get(&id).map(|s| s.as_str()))
    };

    // Photos that exist in the DB but not on disk are removed, the files that have no photo are new
    let (existing_photos, removed_photos): (Vec<_>, Vec<_>) =
        existing_photos.into_iter().partition(|photo| {
            disk_entries_with_name.contains_key(&photo.full_name(folder_name(photo)))
        });
    let existing_photos_names: HashSet<String> = existing_photos
        .iter()
        .map(|photo| photo.full_name(folder_name(photo)))
        .collect();

    let new_photos: Vec<(Photo, Option<String>)> = disk_entries_with_name
        .into_par_iter()
        .filter(|(full_name, _)| !existing_photos_names.contains(full_name))
        .filter_map(|(_, entry)| parse_image(user_id, &entry))
        .collect();

    info!(
        "User {}: found {} new photos, {} removed photos",
        user_id.unwrap_or(PUBLIC_USER_FOLDER),
        new_photos.len(),
        removed_photos.len()
    );

    UserScan {
        new_photos,
        removed_photos,
    }
}

pub fn parse_image(user_id: Option<&str>, entry: &DirEntry) -> Option<(Photo, Option<String>)> {
    let path = entry.path();

    if let Some(timestamp) = timestamp_parsing::get_timestamp_for_path(path) {
        let file_size = entry.metadata().map_or(0i64, |data| data.len() as i64);
        let folder_name = get_folder_name(entry);

        let photo = Photo {
            id: 0,
            user_id: user_id.map(ToOwned::to_owned),
            name: entry.file_name().to_string_lossy().to_string(),
            created_at: timestamp,
            file_size,
            folder_id: None,
            thumb_hash: None,
            trashed_on: None,
        };

        Some((photo, folder_name))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::AppState;
    use crate::repo::tests::{create_test_folder, insert_photo, insert_users, photo};
    use crate::utils::storage_resolver::StorageResolver;
    use sqlx::SqlitePool;

    const TAKEN: &str = r#"{"photoTakenTime": {"timestamp": "1717236900"}}"#;

    struct Setup {
        state: AppStateRef,
        pool: SqlitePool,
        dir: tempfile::TempDir,
    }

    impl Setup {
        async fn new(pool: SqlitePool) -> Self {
            insert_users(&pool).await;
            let dir = tempfile::tempdir().unwrap();
            let storage =
                StorageResolver::new(dir.path().join("storage"), dir.path().join("previews"));
            let state = Box::leak(Box::new(AppState::new(pool.clone(), pool.clone(), storage)));
            Self { state, pool, dir }
        }

        /// Writes `<user>/[<folder>/]<name>` with a sidecar that gives it a timestamp
        fn write(&self, user: &str, folder: Option<&str>, name: &str) {
            let mut path = self.dir.path().join("storage").join(user);
            path.extend(folder);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join(name), name).unwrap();
            std::fs::write(path.join(format!("{name}.json")), TAKEN).unwrap();
        }

        /// `user/[folder/]name` of every row, sorted
        async fn rows(&self) -> Vec<String> {
            let mut rows: Vec<String> = sqlx::query_scalar(
                "select coalesce(p.user_id, 'public') || '/' || coalesce(f.name || '/', '') || p.name
                from photos p left join folders f on f.id = p.folder_id",
            )
            .fetch_all(&self.pool)
            .await
            .unwrap();
            rows.sort();
            rows
        }

        async fn count(&self, sql: &str) -> i64 {
            sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
                .fetch_one(&self.pool)
                .await
                .unwrap()
        }
    }

    #[sqlx::test]
    async fn test_scan_adds_new_files_and_removes_deleted_ones(pool: SqlitePool) {
        let setup = Setup::new(pool).await;
        setup.write("alice", None, "kept.jpg");
        setup.write("alice", None, "new.jpg");
        setup.write("alice", Some("trip"), "in_folder.jpg");
        setup.write("public", None, "family.jpg");
        for name in ["kept.jpg", "gone.jpg"] {
            insert_photo(&setup.pool, photo(Some("alice"), name)).await;
        }
        let gone: i64 = setup
            .count("select id from photos where name = 'gone.jpg'")
            .await;

        scan_new_files(setup.state).await.unwrap();

        let expected = [
            "alice/kept.jpg",
            "alice/new.jpg",
            "alice/trip/in_folder.jpg",
            "public/family.jpg",
        ];
        assert_eq!(setup.rows().await, expected);
        let sql = format!(
            "select count(*) from photos_event_log where photo_id = {gone} and data is null"
        );
        assert_eq!(setup.count(&sql).await, 1);
        let sql = "select count(*) from folder_event_log e join folders f on f.id = e.folder_id
            where f.name = 'trip' and e.data is not null";
        assert_eq!(setup.count(sql).await, 1);
        let sql = "select created_at from photos where name = 'new.jpg'";
        let created_at: String = sqlx::query_scalar(sql)
            .fetch_one(&setup.pool)
            .await
            .unwrap();
        assert_eq!(created_at, "2024-06-01T10:15:00Z");

        // Nothing changed: nothing is written
        let events = setup.count("select count(*) from photos_event_log").await;
        scan_new_files(setup.state).await.unwrap();
        assert_eq!(setup.rows().await, expected);
        assert_eq!(
            setup.count("select count(*) from photos_event_log").await,
            events
        );
    }

    async fn walk(setup: &Setup) -> UserScan {
        scan_user_files(setup.state, Some("alice")).await.unwrap()
    }

    async fn apply(setup: &Setup, scan: UserScan) {
        apply_user_scan(setup.state, Some("alice"), scan)
            .await
            .unwrap();
    }

    /// A walk takes a while, so a second scan can have finished before its result is applied
    #[sqlx::test]
    async fn test_two_scans_do_not_duplicate(pool: SqlitePool) {
        let setup = Setup::new(pool).await;
        setup.write("alice", None, "a.jpg");
        setup.write("alice", Some("trip"), "b.jpg");
        insert_photo(&setup.pool, photo(Some("alice"), "gone.jpg")).await;

        let (first, second) = (walk(&setup).await, walk(&setup).await);
        apply(&setup, first).await;
        apply(&setup, second).await;

        assert_eq!(setup.rows().await, ["alice/a.jpg", "alice/trip/b.jpg"]);
    }

    /// Between the walk and the write, an upload inserts a file, a photo is moved to another
    /// folder and a deleted file comes back
    #[sqlx::test]
    async fn test_stale_scan_keeps_what_changed_meanwhile(pool: SqlitePool) {
        let setup = Setup::new(pool).await;
        setup.write("alice", None, "uploaded.jpg");
        setup.write("alice", None, "scanned.jpg");
        let moved = insert_photo(&setup.pool, photo(Some("alice"), "moved.jpg")).await;
        insert_photo(&setup.pool, photo(Some("alice"), "reappeared.jpg")).await;
        insert_photo(&setup.pool, photo(Some("alice"), "removed.jpg")).await;

        let stale = walk(&setup).await;
        assert_eq!(stale.new_photos.len(), 2);
        assert_eq!(stale.removed_photos.len(), 3);

        insert_photo(&setup.pool, photo(Some("alice"), "uploaded.jpg")).await;
        let folder = create_test_folder(&setup.pool, Some("alice"), "trip").await;
        sqlx::query("update photos set folder_id = ? where id = ?")
            .bind(folder.id)
            .bind(moved.id)
            .execute(&setup.pool)
            .await
            .unwrap();
        setup.write("alice", None, "reappeared.jpg");

        apply(&setup, stale).await;

        assert_eq!(
            setup.rows().await,
            [
                "alice/reappeared.jpg",
                "alice/scanned.jpg",
                "alice/trip/moved.jpg",
                "alice/uploaded.jpg",
            ]
        );
    }
}
