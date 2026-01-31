//! Shared test fixtures and integration tests for repository layer
//!
//! This module provides:
//! - Shared helpers for creating test pools and test data
//! - Integration tests for cross-repo scenarios

use crate::model::photo::Photo;
use crate::model::photo_category::PhotoCategory;
use crate::model::user::User;
use sqlx::SqlitePool;
use time::OffsetDateTime;

/// Create a test photo with the given parameters
pub fn create_test_photo(
    id: i64,
    user_id: Option<&str>,
    folder: Option<&str>,
    name: &str,
) -> Photo {
    Photo {
        id,
        user_id: user_id.map(String::from),
        name: name.to_string(),
        created_at: OffsetDateTime::now_utc(),
        file_size: 1024,
        folder: folder.map(String::from),
        thumb_hash: None,
        trashed_on: None,
    }
}

/// Create a test photo with a specific created_at time
pub fn create_test_photo_with_time(
    id: i64,
    user_id: Option<&str>,
    folder: Option<&str>,
    name: &str,
    created_at: OffsetDateTime,
) -> Photo {
    Photo {
        id,
        user_id: user_id.map(String::from),
        name: name.to_string(),
        created_at,
        file_size: 1024,
        folder: folder.map(String::from),
        thumb_hash: None,
        trashed_on: None,
    }
}

/// Create a test user with the given parameters
pub fn create_test_user(id: &str, display_name: &str) -> User {
    User {
        id: id.to_string(),
        name: display_name.to_string(),
        password_hash: "$argon2id$v=19$m=19456,t=2,p=1$test$testhash".to_string(),
    }
}

/// Insert a test user into the database
pub async fn insert_test_user(pool: &SqlitePool, user: &User) -> sqlx::Result<()> {
    sqlx::query!(
        "insert into users (id, name, password_hash) values ($1, $2, $3)",
        user.id,
        user.name,
        user.password_hash
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Integration tests for cross-repo scenarios
#[cfg(test)]
mod integration {
    use super::*;
    use crate::repo::{FavoritesRepo, PhotosRepo, PhotosTransactionRepo, UserEventLogError};
    use time::macros::datetime;

    #[sqlx::test]
    async fn test_photo_lifecycle(pool: SqlitePool) -> sqlx::Result<()> {
        // Setup: create user
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let mut tx = pool.begin().await?;

        // Insert a photo
        let photo = create_test_photo(0, Some("user1"), Some("vacation"), "beach.jpg");
        let inserted = tx.insert_photo(&photo).await?;
        assert!(inserted.id > 0);
        assert_eq!(inserted.name, "beach.jpg");

        // Update the photo
        let mut updated_photo = inserted.clone();
        updated_photo.folder = Some("summer_vacation".to_string());
        tx.update_photo(&updated_photo).await?;

        tx.commit().await?;

        // Verify update persisted
        let fetched = pool.get_photo(inserted.id, "user1").await?;
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().folder, Some("summer_vacation".to_string()));

        // Add to favorites
        pool.insert_favorite(inserted.id, "user1").await?;
        let is_fav = pool.check_favorite(inserted.id, "user1").await?;
        assert!(is_fav);

        // Remove favorite before deleting (foreign key constraint)
        pool.delete_favorite(inserted.id, "user1").await?;

        // Delete the photo
        let mut tx = pool.begin().await?;
        let deleted_count = tx.delete_photo(&updated_photo).await?;
        tx.commit().await?;
        assert_eq!(deleted_count, 1);

        // Verify deletion
        let fetched_after_delete = pool.get_photo(inserted.id, "user1").await?;
        assert!(fetched_after_delete.is_none());

        Ok(())
    }

    #[sqlx::test]
    async fn test_pagination_with_favorites(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let mut tx = pool.begin().await?;

        // Insert multiple photos with different timestamps
        let photos = vec![
            create_test_photo_with_time(
                0,
                Some("user1"),
                None,
                "photo1.jpg",
                datetime!(2024-01-15 10:00:00 UTC),
            ),
            create_test_photo_with_time(
                0,
                Some("user1"),
                None,
                "photo2.jpg",
                datetime!(2024-01-14 10:00:00 UTC),
            ),
            create_test_photo_with_time(
                0,
                Some("user1"),
                None,
                "photo3.jpg",
                datetime!(2024-01-13 10:00:00 UTC),
            ),
        ];

        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        // Get all photos for user
        let all_photos = pool.get_photos_by_user(Some("user1")).await?;
        assert_eq!(all_photos.len(), 3);

        // Favorite the second photo
        pool.insert_favorite(all_photos[1].id, "user1").await?;

        // Get paginated photos
        let mut tx = pool.begin().await?;
        let paginated = tx
            .get_photos_paginated("user1", PhotoCategory::All, None, 2)
            .await?;
        tx.commit().await?;

        assert_eq!(paginated.photos.len(), 2);
        assert!(paginated.has_more);
        assert!(paginated.next_cursor.is_some());

        // Check favorites
        let fav_ids = pool.get_favorite_photos("user1").await?;
        assert_eq!(fav_ids.len(), 1);
        assert!(fav_ids.contains(&all_photos[1].id));

        Ok(())
    }

    #[sqlx::test]
    async fn test_sync_events_after_modifications(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let mut tx = pool.begin().await?;

        // Insert a photo
        let photo = create_test_photo(0, Some("user1"), None, "test.jpg");
        let inserted = tx.insert_photo(&photo).await?;
        tx.commit().await?;

        // Get the full list with event_log_id
        let mut tx = pool.begin().await?;
        let full_list = tx.get_photos_by_user_and_public("user1").await?;
        let last_event_id = full_list.event_log_id;
        tx.commit().await?;

        // Make more modifications
        let mut tx = pool.begin().await?;
        let mut modified = inserted.clone();
        modified.name = "updated.jpg".to_string();
        tx.update_photo(&modified).await?;
        tx.commit().await?;

        // Get events since the last sync
        let mut tx = pool.begin().await?;
        let events = tx
            .get_events_for_user(last_event_id, "user1")
            .await
            .expect("Should get events after valid event_id");
        tx.commit().await?;

        assert!(!events.events.is_empty());
        assert!(events.event_log_id > last_event_id);

        Ok(())
    }

    #[sqlx::test]
    async fn test_trash_and_restore_flow(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let mut tx = pool.begin().await?;
        let photo = create_test_photo(0, Some("user1"), None, "test.jpg");
        let inserted = tx.insert_photo(&photo).await?;
        tx.commit().await?;

        // Trash the photo
        let mut tx = pool.begin().await?;
        let mut trashed = inserted.clone();
        trashed.id = inserted.id;
        trashed.trashed_on = Some(OffsetDateTime::now_utc());
        tx.update_photo(&trashed).await?;
        tx.commit().await?;

        // Verify photo is trashed
        let fetched = pool.get_photo(inserted.id, "user1").await?;
        assert!(fetched.is_some());
        assert!(fetched.as_ref().unwrap().trashed_on.is_some());

        // Restore the photo
        let mut tx = pool.begin().await?;
        let mut restored = trashed.clone();
        restored.trashed_on = None;
        tx.update_photo(&restored).await?;
        tx.commit().await?;

        // Verify photo is restored
        let fetched = pool.get_photo(inserted.id, "user1").await?;
        assert!(fetched.is_some());
        assert!(fetched.as_ref().unwrap().trashed_on.is_none());

        Ok(())
    }

    #[sqlx::test]
    async fn test_event_log_bounds(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        // Insert a photo to create events first
        let mut tx = pool.begin().await?;
        let photo = create_test_photo(0, Some("user1"), None, "test.jpg");
        tx.insert_photo(&photo).await?;
        tx.commit().await?;

        // Now we have events, test invalid IDs

        // Invalid event ID (too low - before first event)
        let mut tx = pool.begin().await?;
        let result = tx.get_events_for_user(-100, "user1").await;
        tx.commit().await?;
        assert!(
            matches!(result, Err(UserEventLogError::InvalidEventId)),
            "Expected InvalidEventId for ID -100, got {:?}",
            result
        );

        // Invalid event ID (too high - after last event)
        let mut tx = pool.begin().await?;
        let result = tx.get_events_for_user(99999, "user1").await;
        tx.commit().await?;
        assert!(
            matches!(result, Err(UserEventLogError::InvalidEventId)),
            "Expected InvalidEventId for ID 99999, got {:?}",
            result
        );

        // Valid event ID (equal to current max) - should return empty events list
        let mut tx = pool.begin().await?;
        let full_list = tx.get_photos_by_user_and_public("user1").await?;
        let valid_id = full_list.event_log_id;
        tx.commit().await?;

        let mut tx = pool.begin().await?;
        let result = tx.get_events_for_user(valid_id, "user1").await;
        tx.commit().await?;
        assert!(result.is_ok(), "Expected Ok for valid event ID");
        assert!(result.unwrap().events.is_empty());

        Ok(())
    }

    #[sqlx::test]
    async fn test_folders_with_counts_and_filters(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let mut tx = pool.begin().await?;

        let photos = vec![
            create_test_photo(0, Some("user1"), Some("personal_folder"), "p1.jpg"),
            create_test_photo(0, Some("user1"), Some("personal_folder"), "p2.jpg"),
            create_test_photo(0, None, Some("family_folder"), "f1.jpg"),
            create_test_photo(0, None, Some("family_folder"), "f2.jpg"),
            create_test_photo(0, None, Some("family_folder"), "f3.jpg"),
        ];

        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        // All folders (now using pool directly since get_folders_with_counts is on PhotosRepo)
        let folders = pool
            .get_folders_with_counts("user1", PhotoCategory::All)
            .await?;
        assert_eq!(folders.len(), 2);

        // Personal only
        let folders = pool
            .get_folders_with_counts("user1", PhotoCategory::Personal)
            .await?;
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "personal_folder");
        assert_eq!(folders[0].photo_count, 2);

        // Family only
        let folders = pool
            .get_folders_with_counts("user1", PhotoCategory::Family)
            .await?;
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "family_folder");
        assert_eq!(folders[0].photo_count, 3);

        Ok(())
    }
}
