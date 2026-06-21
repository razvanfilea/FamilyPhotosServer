use crate::model::event_log::{EventLog, EventLogs};
use crate::model::photo::{FullPhotosList, Photo, PhotoWithFolder};
use crate::repo::event_log::EventLogRepo;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sqlx::{
    FromRow, QueryBuilder, Sqlite, SqliteExecutor, SqliteTransaction, query, query_as, query_scalar,
};
use thiserror::Error;
use time::OffsetDateTime;

struct PhotoWithFolderRow {
    id: i64,
    user_id: Option<String>,
    name: String,
    created_at: OffsetDateTime,
    file_size: i64,
    folder_id: Option<i64>,
    thumb_hash: Option<Vec<u8>>,
    trashed_on: Option<OffsetDateTime>,
    folder_name: Option<String>,
}

impl PhotoWithFolderRow {
    fn into_photo_with_folder(self) -> PhotoWithFolder {
        PhotoWithFolder {
            photo: Photo {
                id: self.id,
                user_id: self.user_id,
                name: self.name,
                created_at: self.created_at,
                file_size: self.file_size,
                folder_id: self.folder_id,
                thumb_hash: self.thumb_hash,
                trashed_on: self.trashed_on,
            },
            folder_name: self.folder_name,
        }
    }
}

pub trait PhotosRepo<'c>: SqliteExecutor<'c> {
    async fn get_accessible_photo(self, id: i64, user_id: &str) -> sqlx::Result<Option<Photo>> {
        query_as!(
            Photo,
            "select * from photos where id = $1 and (user_id is null or user_id = $2)",
            id,
            user_id
        )
        .fetch_optional(self)
        .await
    }

    async fn get_photo(self, id: i64) -> sqlx::Result<Option<Photo>> {
        query_as!(Photo, "select * from photos where id = $1", id)
            .fetch_optional(self)
            .await
    }

    async fn get_all_photo_ids(self) -> sqlx::Result<Vec<i64>> {
        query_scalar!("select id from photos").fetch_all(self).await
    }

    async fn get_photos_by_user(self, user_id: Option<&str>) -> sqlx::Result<Vec<Photo>> {
        query_as!(
            Photo,
            "select * from photos where (($1 is null and user_id is null) or user_id = $1) order by created_at desc",
            user_id
        )
            .fetch_all(self)
            .await
    }

    async fn get_photo_ids_in_folder(
        self,
        user_id: Option<&str>,
        folder_id: i64,
    ) -> sqlx::Result<Vec<i64>> {
        query_scalar!(
            "select id from photos where (($1 is null and user_id is null) or user_id = $1) and folder_id = $2 order by created_at desc",
            user_id,
            folder_id,
        ).fetch_all(self).await
    }

    async fn get_photos_in_folder(self, folder_id: i64) -> sqlx::Result<Vec<Photo>> {
        query_as!(
            Photo,
            "select * from photos where folder_id = $1 and trashed_on is null order by created_at desc",
            folder_id
        )
        .fetch_all(self)
        .await
    }

    async fn get_photos_with_same_location(self) -> sqlx::Result<Vec<Photo>> {
        query_as!(
            Photo,
            "select * from photos
            where rowid not in (
                select min(rowid)
                from photos
                group by user_id, folder_id, name)",
        )
        .fetch_all(self)
        .await
    }

    async fn get_expired_trash_photos(self) -> sqlx::Result<Vec<Photo>> {
        query_as!(Photo, "select * from photos where trashed_on is not null and trashed_on <= datetime('now', '-30 days')")
            .fetch_all(self)
            .await
    }

    async fn get_photos_without_thumb_hash(self) -> sqlx::Result<Vec<Photo>> {
        query_as!(Photo, "select * from photos where thumb_hash is null")
            .fetch_all(self)
            .await
    }

    async fn get_accessible_photo_with_folder(
        self,
        id: i64,
        user_id: &str,
    ) -> sqlx::Result<Option<PhotoWithFolder>> {
        query_as!(
            PhotoWithFolderRow,
            r#"select p.*, f.name as "folder_name: String"
            from photos p
            left join folders f on f.id = p.folder_id
            where p.id = $1 and (p.user_id is null or p.user_id = $2)"#,
            id,
            user_id
        )
        .fetch_optional(self)
        .await
        .map(|opt| opt.map(PhotoWithFolderRow::into_photo_with_folder))
    }

    async fn get_photo_with_folder(self, photo_id: i64) -> sqlx::Result<Option<PhotoWithFolder>> {
        query_as!(
            PhotoWithFolderRow,
            r#"select p.*, f.name as "folder_name: String"
            from photos p
            left join folders f on f.id = p.folder_id
            where p.id = $1"#,
            photo_id
        )
        .fetch_optional(self)
        .await
        .map(|opt| opt.map(PhotoWithFolderRow::into_photo_with_folder))
    }

    #[allow(dead_code)] // TODO: used by future token-based public link access
    async fn get_photo_in_shared_folder(
        self,
        photo_id: i64,
        folder_id: i64,
    ) -> sqlx::Result<Option<Photo>> {
        query_as!(
            Photo,
            "select * from photos where id = $1 and folder_id = $2",
            photo_id,
            folder_id
        )
        .fetch_optional(self)
        .await
    }
}

impl<'c, E> PhotosRepo<'c> for E where E: SqliteExecutor<'c> {}

pub trait PhotosTransactionRepo<'c> {
    async fn get_photos_by_user_and_public(
        &mut self,
        user_id: &str,
    ) -> sqlx::Result<FullPhotosList>;
    async fn get_events_for_user(
        &mut self,
        last_event_id: i64,
        user_id: &str,
    ) -> Result<EventLogs, UserEventLogError>;
    /// photo.id is ignored
    async fn insert_photo(&mut self, photo: &Photo) -> sqlx::Result<Photo>;
    /// photo.id is ignored
    async fn insert_photos(&mut self, photos: &[Photo]) -> sqlx::Result<()>;
    async fn update_photo(&mut self, photo: &Photo) -> sqlx::Result<()>;
    async fn update_thumb_hashes(&mut self, photos: &[(i64, Vec<u8>)]) -> sqlx::Result<()>;
    async fn delete_photo(&mut self, photo: &Photo) -> sqlx::Result<u64>;
    async fn delete_photos(&mut self, photo_ids: &[i64]) -> sqlx::Result<u64>;
}

impl<'c> PhotosTransactionRepo<'c> for SqliteTransaction<'c> {
    async fn get_photos_by_user_and_public(
        &mut self,
        user_id: &str,
    ) -> sqlx::Result<FullPhotosList> {
        let lastest_event_id = query_scalar!("select max(event_id) from photos_event_log")
            .fetch_one(self.as_mut())
            .await?
            .unwrap_or_default();

        let photos = query_as!(
            Photo,
            "select * from photos where user_id is null or user_id = $1 order by created_at desc",
            user_id,
        )
        .fetch_all(self.as_mut())
        .await?;

        Ok(FullPhotosList {
            event_log_id: lastest_event_id,
            photos,
        })
    }

    // TODO move to a service?
    async fn get_events_for_user(
        &mut self,
        last_event_id: i64,
        user_id: &str,
    ) -> Result<EventLogs, UserEventLogError> {
        let ids = query!(
            "select min(event_id) as 'min_id!: i64', max(event_id) as 'max_id!: i64' from photos_event_log",
        ).map(|record| (record.min_id, record.max_id))
            .fetch_optional(self.as_mut()).await?;

        let Some((min_event_id, max_event_id)) = ids else {
            return Err(UserEventLogError::NoEvents);
        };

        if last_event_id < min_event_id || last_event_id > max_event_id {
            return Err(UserEventLogError::InvalidEventId);
        }

        let event_logs = query!(
            "select photo_id, data from photos_event_log where event_id > $1 and (user_id = $2 or user_id is null) order by event_id",
            last_event_id,
            user_id,
        )
            .map(|record| EventLog {
                photo_id: record.photo_id,
                data: record.data.map(|bytes| STANDARD.encode(bytes))
            })
            .fetch_all(self.as_mut())
            .await?;

        Ok(EventLogs {
            event_log_id: max_event_id,
            events: event_logs,
        })
    }

    async fn insert_photo(&mut self, photo: &Photo) -> sqlx::Result<Photo> {
        let photo = query_as!(
            Photo,
            "insert into photos (user_id, name, created_at, file_size, folder_id, trashed_on) values ($1, $2, $3, $4, $5, $6) returning *",
            photo.user_id,
            photo.name,
            photo.created_at,
            photo.file_size,
            photo.folder_id,
            photo.trashed_on
        )
            .fetch_one(self.as_mut())
            .await?;

        self.insert_event_log(photo.id, photo.user_id.as_deref(), Some(&photo))
            .await?;

        Ok(photo)
    }

    /// photo.id is ignored
    async fn insert_photos(&mut self, photos: &[Photo]) -> sqlx::Result<()> {
        if photos.is_empty() {
            return Ok(());
        }

        let photos = QueryBuilder::<Sqlite>::new(
            "insert into photos (user_id, name, created_at, file_size, folder_id, trashed_on, thumb_hash) ",
        )
            .push_values(photos, |mut b, photo| {
                b.push_bind(&photo.user_id)
                    .push_bind(&photo.name)
                    .push_bind(photo.created_at)
                    .push_bind(photo.file_size)
                    .push_bind(photo.folder_id)
                    .push_bind(photo.trashed_on)
                    .push_bind(&photo.thumb_hash);
            })
            .push(" returning *")
            .build()
            .try_map(|row| Photo::from_row(&row))
            .fetch_all(self.as_mut())
            .await?;

        self.insert_creation_event_logs(&photos).await
    }

    /// Thumb hash is purposely left out, as [`Self::update_thumb_hashes`] exists
    async fn update_photo(&mut self, photo: &Photo) -> sqlx::Result<()> {
        query!(
            "update photos set user_id = $2, name = $3, created_at = $4, file_size = $5, folder_id = $6, trashed_on = $7 where id = $1",
            photo.id,
            photo.user_id,
            photo.name,
            photo.created_at,
            photo.file_size,
            photo.folder_id,
            photo.trashed_on
        )
            .execute(self.as_mut())
            .await?;

        self.insert_event_log(photo.id, photo.user_id.as_deref(), Some(photo))
            .await
    }

    async fn update_thumb_hashes(&mut self, photos: &[(i64, Vec<u8>)]) -> sqlx::Result<()> {
        if photos.is_empty() {
            return Ok(());
        }

        // Batch update using case when to update all photos in a single query
        let mut qb: QueryBuilder<Sqlite> =
            QueryBuilder::new("update photos set thumb_hash = case id ");
        for (id, thumb_hash) in photos {
            qb.push("when ")
                .push_bind(*id)
                .push(" then ")
                .push_bind(thumb_hash)
                .push(" ");
        }
        qb.push("end where id in (");
        let mut sep = qb.separated(", ");
        for (id, _) in photos {
            sep.push_bind(*id);
        }
        sep.push_unseparated(")");
        qb.build().execute(self.as_mut()).await?;

        // Fetch updated photos for event log
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new("select * from photos where id in (");
        let mut sep = qb.separated(", ");
        for (id, _) in photos {
            sep.push_bind(*id);
        }
        sep.push_unseparated(")");
        let updated_photos: Vec<Photo> = qb.build_query_as().fetch_all(self.as_mut()).await?;

        self.insert_creation_event_logs(&updated_photos).await
    }

    async fn delete_photo(&mut self, photo: &Photo) -> sqlx::Result<u64> {
        let rows_deleted = query!("delete from photos where id = $1", photo.id)
            .execute(self.as_mut())
            .await
            .map(|result| result.rows_affected())?;

        self.insert_event_log(photo.id, photo.user_id.as_deref(), None)
            .await?;

        Ok(rows_deleted)
    }

    async fn delete_photos(&mut self, photo_ids: &[i64]) -> sqlx::Result<u64> {
        if photo_ids.is_empty() {
            // But an empty vector would cause a SQL syntax error
            return Ok(0);
        }

        let mut query_builder: QueryBuilder<Sqlite> =
            QueryBuilder::new("delete from photos where id in (");

        let mut separated = query_builder.separated(", ");
        for photo_id in photo_ids.iter() {
            separated.push_bind(photo_id);
        }
        separated.push_unseparated(") ");

        let rows_deleted = query_builder
            .build()
            .execute(self.as_mut())
            .await?
            .rows_affected();

        self.insert_deletion_event_logs(photo_ids).await?;

        Ok(rows_deleted)
    }
}

#[derive(Debug, Error)]
pub enum UserEventLogError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("Invalid event id parameter")]
    InvalidEventId,
    #[error("No events found for user id")]
    NoEvents,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::tests::{
        create_test_folder, create_test_photo, create_test_photo_with_time, create_test_user,
        insert_test_user,
    };
    use sqlx::SqlitePool;
    use time::macros::datetime;

    // ==================== PhotosRepo trait tests ====================

    #[sqlx::test]
    async fn test_get_accessible_photo(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        let user2 = create_test_user("user2", "Other User");
        insert_test_user(&pool, &user).await?;
        insert_test_user(&pool, &user2).await?;

        // Non-existent ID → None
        let result = pool.get_accessible_photo(999, "user1").await?;
        assert!(result.is_none());

        let mut tx = pool.begin().await?;
        let private_photo = create_test_photo(0, Some("user1"), None, "private.jpg");
        let public_photo = create_test_photo(0, None, None, "public.jpg");
        let other_private = create_test_photo(0, Some("user2"), None, "other_private.jpg");

        let private = tx.insert_photo(&private_photo).await?;
        let public = tx.insert_photo(&public_photo).await?;
        let other = tx.insert_photo(&other_private).await?;
        tx.commit().await?;

        // Photo owned by requesting user → Some(photo)
        let result = pool.get_accessible_photo(private.id, "user1").await?;
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "private.jpg");

        // Public photo (user_id=NULL) → accessible by any user
        let result = pool.get_accessible_photo(public.id, "user1").await?;
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "public.jpg");

        let result = pool.get_accessible_photo(public.id, "user2").await?;
        assert!(result.is_some());

        // Private photo owned by different user → None (denied)
        let result = pool.get_accessible_photo(other.id, "user1").await?;
        assert!(result.is_none());

        // Test get_photo: bypasses user ownership check
        // Non-existent → None
        let result = pool.get_photo(999).await?;
        assert!(result.is_none());

        // Any existing photo → Some(photo) regardless of ownership
        let result = pool.get_photo(other.id).await?;
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "other_private.jpg");

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_all_photo_ids(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        // Empty DB → empty vec
        let ids = pool.get_all_photo_ids().await?;
        assert!(ids.is_empty());

        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), None, "p1.jpg"),
            create_test_photo(0, None, None, "public.jpg"),
        ];
        tx.insert_photos(&photos).await?;

        // Trash one photo
        let mut trashed = create_test_photo(0, Some("user1"), None, "trashed.jpg");
        trashed.trashed_on = Some(OffsetDateTime::now_utc());
        tx.insert_photo(&trashed).await?;
        tx.commit().await?;

        // Multiple photos (public + private + trashed) → all IDs returned
        let ids = pool.get_all_photo_ids().await?;
        assert_eq!(ids.len(), 3);

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_photos_by_user(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        // Empty result when no matches
        let photos = pool.get_photos_by_user(Some("user1")).await?;
        assert!(photos.is_empty());

        let mut tx = pool.begin().await?;
        let photos_to_insert = vec![
            create_test_photo_with_time(
                0,
                Some("user1"),
                None,
                "p1.jpg",
                datetime!(2024-01-15 10:00:00 UTC),
            ),
            create_test_photo_with_time(
                0,
                Some("user1"),
                None,
                "p2.jpg",
                datetime!(2024-01-14 10:00:00 UTC),
            ),
            create_test_photo_with_time(
                0,
                None,
                None,
                "public.jpg",
                datetime!(2024-01-13 10:00:00 UTC),
            ),
        ];
        tx.insert_photos(&photos_to_insert).await?;
        tx.commit().await?;

        // user_id=Some → only that user's photos
        let photos = pool.get_photos_by_user(Some("user1")).await?;
        assert_eq!(photos.len(), 2);
        // Verify ordering by created_at DESC
        assert_eq!(photos[0].name, "p1.jpg");
        assert_eq!(photos[1].name, "p2.jpg");

        // user_id=None → only public photos
        let photos = pool.get_photos_by_user(None).await?;
        assert_eq!(photos.len(), 1);
        assert_eq!(photos[0].name, "public.jpg");

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_photo_ids_in_folder(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let vacation = create_test_folder(&pool, Some("user1"), "vacation").await;
        let public_vacation = create_test_folder(&pool, None, "vacation").await;
        let other = create_test_folder(&pool, Some("user1"), "other").await;

        // Non-existent folder_id → empty vec
        let ids = pool.get_photo_ids_in_folder(Some("user1"), 99999).await?;
        assert!(ids.is_empty());

        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), Some(vacation.id), "v1.jpg"),
            create_test_photo(0, Some("user1"), Some(vacation.id), "v2.jpg"),
            create_test_photo(0, None, Some(public_vacation.id), "public_v.jpg"),
            create_test_photo(0, Some("user1"), Some(other.id), "o1.jpg"),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        // user_id=Some + folder exists → that user's photos in folder
        let ids = pool
            .get_photo_ids_in_folder(Some("user1"), vacation.id)
            .await?;
        assert_eq!(ids.len(), 2);

        // user_id=None + folder exists → public photos in folder
        let ids = pool
            .get_photo_ids_in_folder(None, public_vacation.id)
            .await?;
        assert_eq!(ids.len(), 1);

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_photos_with_same_location(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        let user2 = create_test_user("user2", "Other User");
        insert_test_user(&pool, &user).await?;
        insert_test_user(&pool, &user2).await?;

        let folder1 = create_test_folder(&pool, Some("user1"), "folder").await;
        let folder2 = create_test_folder(&pool, Some("user2"), "folder").await;

        // No duplicates → empty vec
        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), Some(folder1.id), "unique1.jpg"),
            create_test_photo(0, Some("user1"), Some(folder1.id), "unique2.jpg"),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        let dupes = pool.get_photos_with_same_location().await?;
        assert!(dupes.is_empty());

        // Duplicates (same user_id+folder_id+name) → returns all but first
        let mut tx = pool.begin().await?;
        let photo = create_test_photo(0, Some("user1"), Some(folder1.id), "unique1.jpg");
        tx.insert_photo(&photo).await?;
        tx.commit().await?;

        let dupes = pool.get_photos_with_same_location().await?;
        assert_eq!(dupes.len(), 1);
        assert_eq!(dupes[0].name, "unique1.jpg");

        // Duplicates across different users → treated separately (not duplicates)
        let mut tx = pool.begin().await?;
        let photo = create_test_photo(0, Some("user2"), Some(folder2.id), "unique1.jpg");
        tx.insert_photo(&photo).await?;
        tx.commit().await?;

        let dupes = pool.get_photos_with_same_location().await?;
        assert_eq!(dupes.len(), 1); // Still just 1, user2's photo is separate

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_expired_trash_photos(pool: SqlitePool) -> sqlx::Result<()> {
        use time::Duration;

        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        // No trashed photos → empty
        let expired = pool.get_expired_trash_photos().await?;
        assert!(expired.is_empty());

        let now = OffsetDateTime::now_utc();
        let mut tx = pool.begin().await?;

        // Photo trashed < 30 days ago → should NOT be returned
        let mut recent_trash = create_test_photo(0, Some("user1"), None, "recent.jpg");
        recent_trash.trashed_on = Some(now - Duration::days(15));
        tx.insert_photo(&recent_trash).await?;

        // Photo trashed > 30 days ago → SHOULD be returned
        let mut old_trash = create_test_photo(0, Some("user1"), None, "old.jpg");
        old_trash.trashed_on = Some(now - Duration::days(35));
        tx.insert_photo(&old_trash).await?;

        tx.commit().await?;

        let expired = pool.get_expired_trash_photos().await?;
        // Should include only the old one (> 30 days)
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].name, "old.jpg");

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_photos_without_thumb_hash(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        // Insert photos using insert_photos which includes thumb_hash
        let mut tx = pool.begin().await?;

        // Photo without thumb_hash
        let photo_no_hash = create_test_photo(0, Some("user1"), None, "no_hash.jpg");

        // Photo with thumb_hash (use insert_photos to include thumb_hash)
        let mut photo_with_hash = create_test_photo(0, Some("user1"), None, "with_hash.jpg");
        photo_with_hash.thumb_hash = Some(vec![1, 2, 3, 4]);

        tx.insert_photos(&[photo_no_hash, photo_with_hash]).await?;
        tx.commit().await?;

        let without_hash = pool.get_photos_without_thumb_hash().await?;
        assert_eq!(without_hash.len(), 1);
        assert_eq!(without_hash[0].name, "no_hash.jpg");

        Ok(())
    }

    // ==================== PhotosTransactionRepo tests ====================

    #[sqlx::test]
    async fn test_get_photos_by_user_and_public(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        let user2 = create_test_user("user2", "Other User");
        insert_test_user(&pool, &user).await?;
        insert_test_user(&pool, &user2).await?;

        // Empty DB → event_log_id=0, empty photos
        let mut tx = pool.begin().await?;
        let result = tx.get_photos_by_user_and_public("user1").await?;
        tx.commit().await?;
        assert_eq!(result.event_log_id, 0);
        assert!(result.photos.is_empty());

        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), None, "user1_photo.jpg"),
            create_test_photo(0, None, None, "public.jpg"),
            create_test_photo(0, Some("user2"), None, "user2_private.jpg"),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        // user's photos + public → both included, other user's private → excluded
        let mut tx = pool.begin().await?;
        let result = tx.get_photos_by_user_and_public("user1").await?;
        tx.commit().await?;

        assert_eq!(result.photos.len(), 2);
        assert!(result.event_log_id > 0);

        let names: Vec<&str> = result.photos.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"user1_photo.jpg"));
        assert!(names.contains(&"public.jpg"));
        assert!(!names.contains(&"user2_private.jpg"));

        Ok(())
    }

    #[sqlx::test]
    async fn test_insert_photo(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        // Public photo → inserted with NULL user_id, event created
        let mut tx = pool.begin().await?;
        let public_photo = create_test_photo(0, None, None, "public.jpg");
        let inserted = tx.insert_photo(&public_photo).await?;
        tx.commit().await?;

        assert!(inserted.id > 0);
        assert!(inserted.user_id.is_none());

        // Private photo → inserted with user_id, event created
        let mut tx = pool.begin().await?;
        let private_photo = create_test_photo(0, Some("user1"), None, "private.jpg");
        let inserted = tx.insert_photo(&private_photo).await?;
        tx.commit().await?;

        assert!(inserted.id > 0);
        assert_eq!(inserted.user_id, Some("user1".to_string()));

        Ok(())
    }

    #[sqlx::test]
    async fn test_insert_photos(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        // Empty vec → Ok, no events created
        let mut tx = pool.begin().await?;
        tx.insert_photos(&[]).await?;
        tx.commit().await?;

        // Multiple photos → all inserted
        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), None, "p1.jpg"),
            create_test_photo(0, None, None, "public.jpg"),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        let all_ids = pool.get_all_photo_ids().await?;
        assert_eq!(all_ids.len(), 2);

        Ok(())
    }

    #[sqlx::test]
    async fn test_update_photo(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let folder1 = create_test_folder(&pool, Some("user1"), "original").await;
        let folder2 = create_test_folder(&pool, Some("user1"), "new_folder").await;

        let mut tx = pool.begin().await?;
        let photo = create_test_photo(0, Some("user1"), Some(folder1.id), "test.jpg");
        let inserted = tx.insert_photo(&photo).await?;
        tx.commit().await?;

        // Update name → persisted, event created
        let mut tx = pool.begin().await?;
        let mut updated = inserted.clone();
        updated.name = "renamed.jpg".to_string();
        tx.update_photo(&updated).await?;
        tx.commit().await?;

        let fetched = pool
            .get_accessible_photo(inserted.id, "user1")
            .await?
            .unwrap();
        assert_eq!(fetched.name, "renamed.jpg");

        // Update folder_id → persisted, event created
        let mut tx = pool.begin().await?;
        updated.folder_id = Some(folder2.id);
        tx.update_photo(&updated).await?;
        tx.commit().await?;

        let fetched = pool
            .get_accessible_photo(inserted.id, "user1")
            .await?
            .unwrap();
        assert_eq!(fetched.folder_id, Some(folder2.id));

        // Set trashed_on → moves to trash, event created
        let mut tx = pool.begin().await?;
        updated.trashed_on = Some(OffsetDateTime::now_utc());
        tx.update_photo(&updated).await?;
        tx.commit().await?;

        let fetched = pool
            .get_accessible_photo(inserted.id, "user1")
            .await?
            .unwrap();
        assert!(fetched.trashed_on.is_some());

        Ok(())
    }

    #[sqlx::test]
    async fn test_update_thumb_hashes(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        // Empty vec → Ok, no events
        let mut tx = pool.begin().await?;
        tx.update_thumb_hashes(&[]).await?;
        tx.commit().await?;

        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), None, "p1.jpg"),
            create_test_photo(0, Some("user1"), None, "p2.jpg"),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        let ids = pool.get_all_photo_ids().await?;

        // Update thumb hashes
        let mut tx = pool.begin().await?;
        let updates = vec![(ids[0], vec![1, 2, 3, 4]), (ids[1], vec![5, 6, 7, 8])];
        tx.update_thumb_hashes(&updates).await?;
        tx.commit().await?;

        // Verify thumb_hash actually changed
        let p1 = pool.get_photo(ids[0]).await?.unwrap();
        let p2 = pool.get_photo(ids[1]).await?.unwrap();
        assert_eq!(p1.thumb_hash, Some(vec![1, 2, 3, 4]));
        assert_eq!(p2.thumb_hash, Some(vec![5, 6, 7, 8]));

        Ok(())
    }

    #[sqlx::test]
    async fn test_delete_photo(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let mut tx = pool.begin().await?;
        let photo = create_test_photo(0, Some("user1"), None, "to_delete.jpg");
        let inserted = tx.insert_photo(&photo).await?;
        tx.commit().await?;

        // Existing photo → deleted, returns 1
        let mut tx = pool.begin().await?;
        let deleted = tx.delete_photo(&inserted).await?;
        tx.commit().await?;
        assert_eq!(deleted, 1);

        // Verify deleted
        let fetched = pool.get_photo(inserted.id).await?;
        assert!(fetched.is_none());

        Ok(())
    }

    #[sqlx::test]
    async fn test_delete_photos(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        // Empty vec → Ok, returns 0
        let mut tx = pool.begin().await?;
        let deleted = tx.delete_photos(&[]).await?;
        tx.commit().await?;
        assert_eq!(deleted, 0);

        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), None, "p1.jpg"),
            create_test_photo(0, Some("user1"), None, "p2.jpg"),
            create_test_photo(0, Some("user1"), None, "p3.jpg"),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        let ids = pool.get_all_photo_ids().await?;

        // Multiple existing → all deleted, correct count
        let mut tx = pool.begin().await?;
        let deleted = tx.delete_photos(&[ids[0], ids[1]]).await?;
        tx.commit().await?;
        assert_eq!(deleted, 2);

        let remaining = pool.get_all_photo_ids().await?;
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0], ids[2]);

        // Mix of existing/non-existent → only existing deleted
        let mut tx = pool.begin().await?;
        let deleted = tx.delete_photos(&[ids[2], 9999]).await?;
        tx.commit().await?;
        assert_eq!(deleted, 1);

        Ok(())
    }
}
