//! Shared test fixtures and integration tests for repository layer
//!
//! This module provides:
//! - Shared helpers for creating test pools and test data
//! - Integration tests for cross-repo scenarios

use crate::model::photo::Photo;
use crate::model::user::User;
use sqlx::SqlitePool;
use time::OffsetDateTime;

/// Create a test photo with the given parameters
pub fn create_test_photo(
    id: i64,
    user_id: Option<&str>,
    folder_id: Option<i64>,
    name: &str,
) -> Photo {
    Photo {
        id,
        user_id: user_id.map(String::from),
        name: name.to_string(),
        created_at: OffsetDateTime::now_utc(),
        file_size: 1024,
        folder_id,
        thumb_hash: None,
        trashed_on: None,
    }
}

/// Create a test photo with a specific created_at time
pub fn create_test_photo_with_time(
    id: i64,
    user_id: Option<&str>,
    folder_id: Option<i64>,
    name: &str,
    created_at: OffsetDateTime,
) -> Photo {
    Photo {
        id,
        user_id: user_id.map(String::from),
        name: name.to_string(),
        created_at,
        file_size: 1024,
        folder_id,
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
    use crate::repo::{PhotosRepo, PhotosTransactionRepo, UserEventLogError};

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
}
