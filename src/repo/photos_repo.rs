use crate::model::event_log::{EventLog, EventLogs};
use crate::model::photo::{FullPhotosList, Photo};
use crate::repo::event_log::EventLogRepo;
use serde::{Deserialize, Serialize};
use sqlx::{
    FromRow, QueryBuilder, Sqlite, SqliteExecutor, SqliteTransaction, query, query_as, query_scalar,
};
use thiserror::Error;
use time::OffsetDateTime;

/// Cursor for cursor-based pagination of photos
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhotoCursor {
    pub created_at: OffsetDateTime,
    pub id: i64,
}

/// Folder with photo count for display
#[derive(Serialize)]
pub struct FolderInfo {
    pub name: String,
    #[serde(rename = "count")]
    pub photo_count: i64,
    pub cover_photo_id: i64,
}

/// Result of a paginated photo query
pub struct PaginatedPhotos {
    pub photos: Vec<Photo>,
    pub next_cursor: Option<PhotoCursor>,
    pub has_more: bool,
}

/// Summary of photos per month for timeline display
pub struct MonthSummary {
    pub year: i32,
    pub month: u8,
    pub count: i64,
}

pub trait PhotosRepo<'c>: SqliteExecutor<'c> {
    async fn get_photo(self, id: i64, user_id: &str) -> sqlx::Result<Option<Photo>> {
        query_as!(
            Photo,
            "select * from photos where id = $1 and (user_id is null or user_id = $2)",
            id,
            user_id
        )
        .fetch_optional(self)
        .await
    }

    async fn get_photo_without_check(self, id: i64) -> sqlx::Result<Option<Photo>> {
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
        folder_name: &str,
    ) -> sqlx::Result<Vec<i64>> {
        query_scalar!(
            "select id from photos where (($1 is null and user_id is null) or user_id = $1) and folder = $2 order by created_at desc",
            user_id,
            folder_name,
        ).fetch_all(self).await
    }

    async fn get_photos_with_same_location(self) -> sqlx::Result<Vec<Photo>> {
        query_as!(
            Photo,
            "select * from photos
            where rowid not in (
                select min(rowid)
                from photos
                group by user_id, folder, name)",
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

    /// Get all trashed photos accessible to a user (user's own + public)
    async fn get_trashed_photos(self, user_id: &str) -> sqlx::Result<Vec<Photo>> {
        query_as!(
            Photo,
            "select * from photos
             where (user_id is null or user_id = $1)
               and trashed_on is not null
             order by trashed_on desc",
            user_id
        )
        .fetch_all(self)
        .await
    }

    /// Get distinct personal folder names for a user
    async fn get_distinct_personal_folders(self, user_id: &str) -> sqlx::Result<Vec<String>> {
        query_scalar!(
            "select distinct folder as 'folder!' from photos
             where user_id = $1
               and folder is not null and folder != ''
               and trashed_on is null
             order by folder",
            user_id
        )
        .fetch_all(self)
        .await
    }

    /// Get distinct family (public) folder names
    async fn get_distinct_family_folders(self) -> sqlx::Result<Vec<String>> {
        query_scalar!(
            "select distinct folder as 'folder!' from photos
             where user_id is null
               and folder is not null and folder != ''
               and trashed_on is null
             order by folder"
        )
        .fetch_all(self)
        .await
    }

    async fn get_photos_paginated(
        self,
        user_id: &str,
        personal_only: bool,
        family_only: bool,
        cursor: Option<&PhotoCursor>,
        limit: u32,
    ) -> sqlx::Result<PaginatedPhotos> {
        // Fetch one extra to determine if there are more results
        let fetch_limit = limit as i64 + 1;

        let photos = if let Some(cursor) = cursor {
            query_as!(
                Photo,
                r#"select * from photos
                where (user_id is null or user_id = $1)
                  and trashed_on is null
                  and (($5 = 0 and $6 = 0) or ($5 = 1 and user_id = $1) or ($6 = 1 and user_id is null))
                  and (created_at < $2 or (created_at = $2 and id < $3))
                order by created_at desc, id desc
                limit $4"#,
                user_id,
                cursor.created_at,
                cursor.id,
                fetch_limit,
                personal_only,
                family_only
            )
            .fetch_all(self)
            .await?
        } else {
            query_as!(
                Photo,
                r#"select * from photos
                where (user_id is null or user_id = $1)
                  and trashed_on is null
                  and (($3 = 0 and $4 = 0) or ($3 = 1 and user_id = $1) or ($4 = 1 and user_id is null))
                order by created_at desc, id desc
                limit $2"#,
                user_id,
                fetch_limit,
                personal_only,
                family_only
            )
            .fetch_all(self)
            .await?
        };

        build_paginated_result(photos, limit)
    }

    async fn get_folder_photos_paginated(
        self,
        user_id: &str,
        folder_name: &str,
        cursor: Option<&PhotoCursor>,
        limit: u32,
    ) -> sqlx::Result<PaginatedPhotos> {
        let fetch_limit = limit as i64 + 1;

        let photos = if let Some(cursor) = cursor {
            query_as!(
                Photo,
                r#"select * from photos
                where (user_id is null or user_id = $1)
                  and trashed_on is null
                  and folder = $2
                  and (created_at < $3 or (created_at = $3 and id < $4))
                order by created_at desc, id desc
                limit $5"#,
                user_id,
                folder_name,
                cursor.created_at,
                cursor.id,
                fetch_limit
            )
            .fetch_all(self)
            .await?
        } else {
            query_as!(
                Photo,
                r#"select * from photos
                where (user_id is null or user_id = $1)
                  and trashed_on is null
                  and folder = $2
                order by created_at desc, id desc
                limit $3"#,
                user_id,
                folder_name,
                fetch_limit
            )
            .fetch_all(self)
            .await?
        };

        build_paginated_result(photos, limit)
    }

    async fn get_favorite_photos_paginated(
        self,
        user_id: &str,
        cursor: Option<&PhotoCursor>,
        limit: u32,
    ) -> sqlx::Result<PaginatedPhotos> {
        let fetch_limit = limit as i64 + 1;

        let photos = if let Some(cursor) = cursor {
            query_as!(
                Photo,
                r#"select p.* from photos p
                inner join favorite_photos f on p.id = f.photo_id and f.user_id = $1
                where (p.user_id is null or p.user_id = $1)
                  and p.trashed_on is null
                  and (p.created_at < $2 or (p.created_at = $2 and p.id < $3))
                order by p.created_at desc, p.id desc
                limit $4"#,
                user_id,
                cursor.created_at,
                cursor.id,
                fetch_limit
            )
            .fetch_all(self)
            .await?
        } else {
            query_as!(
                Photo,
                r#"select p.* from photos p
                inner join favorite_photos f on p.id = f.photo_id and f.user_id = $1
                where (p.user_id is null or p.user_id = $1)
                  and p.trashed_on is null
                order by p.created_at desc, p.id desc
                limit $2"#,
                user_id,
                fetch_limit
            )
            .fetch_all(self)
            .await?
        };

        build_paginated_result(photos, limit)
    }

    async fn get_folders_with_counts(
        self,
        user_id: &str,
        personal_only: bool,
        family_only: bool,
    ) -> sqlx::Result<Vec<FolderInfo>> {
        let rows = query!(
            r#"select
                folder as "name!",
                count(*) as "photo_count!: i64",
                (select id from photos p2
                 where p2.folder = photos.folder
                   and p2.trashed_on is null
                   and (p2.user_id is null or p2.user_id = $1)
                   and (($2 = 0 and $3 = 0) or ($2 = 1 and p2.user_id = $1) or ($3 = 1 and p2.user_id is null))
                 order by p2.created_at desc limit 1) as "cover_photo_id!: i64"
            from photos
            where (user_id is null or user_id = $1)
              and trashed_on is null
              and folder is not null and folder != ''
              and (($2 = 0 and $3 = 0) or ($2 = 1 and user_id = $1) or ($3 = 1 and user_id is null))
            group by folder
            order by folder asc"#,
            user_id,
            personal_only,
            family_only
        )
        .fetch_all(self)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| FolderInfo {
                name: r.name,
                photo_count: r.photo_count,
                cover_photo_id: r.cover_photo_id,
            })
            .collect())
    }

    async fn get_month_summaries(
        self,
        user_id: &str,
        personal_only: bool,
        family_only: bool,
    ) -> sqlx::Result<Vec<MonthSummary>> {
        let rows = query!(
            r#"select
                cast(strftime('%Y', created_at) as integer) as "year!: i32",
                cast(strftime('%m', created_at) as integer) as "month!: i32",
                count(*) as "count!: i64"
            from photos
            where (user_id is null or user_id = $1)
              and trashed_on is null
              and (($2 = 0 and $3 = 0) or ($2 = 1 and user_id = $1) or ($3 = 1 and user_id is null))
            group by 1, 2
            order by 1 desc, 2 desc"#,
            user_id,
            personal_only,
            family_only
        )
        .fetch_all(self)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| MonthSummary {
                year: r.year,
                month: r.month as u8,
                count: r.count,
            })
            .collect())
    }

    async fn get_folder_month_summaries(
        self,
        user_id: &str,
        folder_name: &str,
    ) -> sqlx::Result<Vec<MonthSummary>> {
        let rows = query!(
            r#"select
                cast(strftime('%Y', created_at) as integer) as "year!: i32",
                cast(strftime('%m', created_at) as integer) as "month!: i32",
                count(*) as "count!: i64"
            from photos
            where (user_id is null or user_id = $1)
              and trashed_on is null
              and folder = $2
            group by 1, 2
            order by 1 desc, 2 desc"#,
            user_id,
            folder_name
        )
        .fetch_all(self)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| MonthSummary {
                year: r.year,
                month: r.month as u8,
                count: r.count,
            })
            .collect())
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
    // TODO move to a service?
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

        let event_logs = query_as!(
            EventLog,
            "select photo_id, data from photos_event_log where event_id > $1 and (user_id = $2 or user_id is null) order by event_id",
            last_event_id,
            user_id,
        )
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
            "insert into photos (user_id, name, created_at, file_size, folder, trashed_on) values ($1, $2, $3, $4, $5, $6) returning *",
            photo.user_id,
            photo.name,
            photo.created_at,
            photo.file_size,
            photo.folder,
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
            "insert into photos (user_id, name, created_at, file_size, folder, trashed_on, thumb_hash) ",
        )
            .push_values(photos, |mut b, photo| {
                b.push_bind(&photo.user_id)
                    .push_bind(&photo.name)
                    .push_bind(photo.created_at)
                    .push_bind(photo.file_size)
                    .push_bind(&photo.folder)
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
            "update photos set user_id = $2, name = $3, created_at = $4, file_size = $5, folder = $6, trashed_on = $7 where id = $1",
            photo.id,
            photo.user_id,
            photo.name,
            photo.created_at,
            photo.file_size,
            photo.folder,
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

fn build_paginated_result(mut photos: Vec<Photo>, limit: u32) -> sqlx::Result<PaginatedPhotos> {
    let has_more = photos.len() > limit as usize;
    if has_more {
        photos.pop(); // Remove the extra photo we fetched
    }

    let next_cursor = if has_more {
        photos.last().map(|p| PhotoCursor {
            created_at: p.created_at,
            id: p.id,
        })
    } else {
        None
    };

    Ok(PaginatedPhotos {
        photos,
        next_cursor,
        has_more,
    })
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
        create_test_photo, create_test_photo_with_time, create_test_user, insert_test_user,
    };
    use sqlx::SqlitePool;
    use time::macros::datetime;

    // ==================== PhotosRepo trait tests ====================

    #[sqlx::test]
    async fn test_get_photo(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        let user2 = create_test_user("user2", "Other User");
        insert_test_user(&pool, &user).await?;
        insert_test_user(&pool, &user2).await?;

        // Non-existent ID → None
        let result = pool.get_photo(999, "user1").await?;
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
        let result = pool.get_photo(private.id, "user1").await?;
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "private.jpg");

        // Public photo (user_id=NULL) → accessible by any user
        let result = pool.get_photo(public.id, "user1").await?;
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "public.jpg");

        let result = pool.get_photo(public.id, "user2").await?;
        assert!(result.is_some());

        // Private photo owned by different user → None (denied)
        let result = pool.get_photo(other.id, "user1").await?;
        assert!(result.is_none());

        // Test get_photo_without_check: bypasses user ownership check
        // Non-existent → None
        let result = pool.get_photo_without_check(999).await?;
        assert!(result.is_none());

        // Any existing photo → Some(photo) regardless of ownership
        let result = pool.get_photo_without_check(other.id).await?;
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

        // Non-existent folder → empty vec
        let ids = pool
            .get_photo_ids_in_folder(Some("user1"), "nonexistent")
            .await?;
        assert!(ids.is_empty());

        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), Some("vacation"), "v1.jpg"),
            create_test_photo(0, Some("user1"), Some("vacation"), "v2.jpg"),
            create_test_photo(0, None, Some("vacation"), "public_v.jpg"),
            create_test_photo(0, Some("user1"), Some("other"), "o1.jpg"),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        // user_id=Some + folder exists → that user's photos in folder
        let ids = pool
            .get_photo_ids_in_folder(Some("user1"), "vacation")
            .await?;
        assert_eq!(ids.len(), 2);

        // user_id=None + folder exists → public photos in folder
        let ids = pool.get_photo_ids_in_folder(None, "vacation").await?;
        assert_eq!(ids.len(), 1);

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_photos_with_same_location(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        let user2 = create_test_user("user2", "Other User");
        insert_test_user(&pool, &user).await?;
        insert_test_user(&pool, &user2).await?;

        // No duplicates → empty vec
        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), Some("folder"), "unique1.jpg"),
            create_test_photo(0, Some("user1"), Some("folder"), "unique2.jpg"),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        let dupes = pool.get_photos_with_same_location().await?;
        assert!(dupes.is_empty());

        // Duplicates (same user_id+folder+name) → returns all but first
        let mut tx = pool.begin().await?;
        let photo = create_test_photo(0, Some("user1"), Some("folder"), "unique1.jpg");
        tx.insert_photo(&photo).await?;
        tx.commit().await?;

        let dupes = pool.get_photos_with_same_location().await?;
        assert_eq!(dupes.len(), 1);
        assert_eq!(dupes[0].name, "unique1.jpg");

        // Duplicates across different users → treated separately (not duplicates)
        let mut tx = pool.begin().await?;
        let photo = create_test_photo(0, Some("user2"), Some("folder"), "unique1.jpg");
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

    #[sqlx::test]
    async fn test_get_trashed_photos(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let mut tx = pool.begin().await?;

        // Non-trashed photo
        let normal = create_test_photo(0, Some("user1"), None, "normal.jpg");
        tx.insert_photo(&normal).await?;

        // Trashed photo
        let mut trashed = create_test_photo(0, Some("user1"), None, "trashed.jpg");
        trashed.trashed_on = Some(OffsetDateTime::now_utc());
        tx.insert_photo(&trashed).await?;

        // Public trashed photo
        let mut public_trashed = create_test_photo(0, None, None, "public_trashed.jpg");
        public_trashed.trashed_on = Some(OffsetDateTime::now_utc());
        tx.insert_photo(&public_trashed).await?;

        tx.commit().await?;

        // Should return only trashed photos accessible to user
        let trashed_photos = pool.get_trashed_photos("user1").await?;
        assert_eq!(trashed_photos.len(), 2);

        // Both trashed photos should be included
        let names: Vec<&str> = trashed_photos.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"trashed.jpg"));
        assert!(names.contains(&"public_trashed.jpg"));
        assert!(!names.contains(&"normal.jpg"));

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_distinct_folders(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), Some("personal_a"), "p1.jpg"),
            create_test_photo(0, Some("user1"), Some("personal_b"), "p2.jpg"),
            create_test_photo(0, Some("user1"), Some("personal_a"), "p3.jpg"), // duplicate folder
            create_test_photo(0, None, Some("family_a"), "f1.jpg"),
            create_test_photo(0, None, Some("family_b"), "f2.jpg"),
            create_test_photo(0, Some("user1"), None, "no_folder.jpg"), // no folder
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        // Personal folders
        let personal = pool.get_distinct_personal_folders("user1").await?;
        assert_eq!(personal.len(), 2);
        assert!(personal.contains(&"personal_a".to_string()));
        assert!(personal.contains(&"personal_b".to_string()));

        // Family folders
        let family = pool.get_distinct_family_folders().await?;
        assert_eq!(family.len(), 2);
        assert!(family.contains(&"family_a".to_string()));
        assert!(family.contains(&"family_b".to_string()));

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

        let mut tx = pool.begin().await?;
        let photo = create_test_photo(0, Some("user1"), Some("original"), "test.jpg");
        let inserted = tx.insert_photo(&photo).await?;
        tx.commit().await?;

        // Update name → persisted, event created
        let mut tx = pool.begin().await?;
        let mut updated = inserted.clone();
        updated.name = "renamed.jpg".to_string();
        tx.update_photo(&updated).await?;
        tx.commit().await?;

        let fetched = pool.get_photo(inserted.id, "user1").await?.unwrap();
        assert_eq!(fetched.name, "renamed.jpg");

        // Update folder → persisted, event created
        let mut tx = pool.begin().await?;
        updated.folder = Some("new_folder".to_string());
        tx.update_photo(&updated).await?;
        tx.commit().await?;

        let fetched = pool.get_photo(inserted.id, "user1").await?.unwrap();
        assert_eq!(fetched.folder, Some("new_folder".to_string()));

        // Set trashed_on → moves to trash, event created
        let mut tx = pool.begin().await?;
        updated.trashed_on = Some(OffsetDateTime::now_utc());
        tx.update_photo(&updated).await?;
        tx.commit().await?;

        let fetched = pool.get_photo(inserted.id, "user1").await?.unwrap();
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
        let p1 = pool.get_photo_without_check(ids[0]).await?.unwrap();
        let p2 = pool.get_photo_without_check(ids[1]).await?.unwrap();
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
        let fetched = pool.get_photo_without_check(inserted.id).await?;
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

    #[sqlx::test]
    async fn test_get_photos_paginated(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo_with_time(
                0,
                Some("user1"),
                None,
                "p1.jpg",
                datetime!(2024-01-05 10:00:00 UTC),
            ),
            create_test_photo_with_time(
                0,
                Some("user1"),
                None,
                "p2.jpg",
                datetime!(2024-01-04 10:00:00 UTC),
            ),
            create_test_photo_with_time(
                0,
                Some("user1"),
                None,
                "p3.jpg",
                datetime!(2024-01-03 10:00:00 UTC),
            ),
            create_test_photo_with_time(
                0,
                None,
                None,
                "public.jpg",
                datetime!(2024-01-02 10:00:00 UTC),
            ),
        ];
        tx.insert_photos(&photos).await?;

        // Add a trashed photo that should be excluded
        let mut trashed = create_test_photo(0, Some("user1"), None, "trashed.jpg");
        trashed.trashed_on = Some(OffsetDateTime::now_utc());
        tx.insert_photo(&trashed).await?;
        tx.commit().await?;

        // No cursor, no filters → first page of all accessible photos
        let mut tx = pool.begin().await?;
        let result = tx
            .get_photos_paginated("user1", false, false, None, 2)
            .await?;
        tx.commit().await?;

        assert_eq!(result.photos.len(), 2);
        assert!(result.has_more);
        assert!(result.next_cursor.is_some());
        assert_eq!(result.photos[0].name, "p1.jpg");
        assert_eq!(result.photos[1].name, "p2.jpg");

        // With cursor → returns photos after cursor
        let cursor = result.next_cursor.as_ref().unwrap();
        let mut tx = pool.begin().await?;
        let result = tx
            .get_photos_paginated("user1", false, false, Some(cursor), 10)
            .await?;
        tx.commit().await?;

        assert_eq!(result.photos.len(), 2); // p3 and public
        assert!(!result.has_more);
        assert!(result.next_cursor.is_none());

        // personal_only=true → only user's private photos
        let mut tx = pool.begin().await?;
        let result = tx
            .get_photos_paginated("user1", true, false, None, 10)
            .await?;
        tx.commit().await?;
        assert_eq!(result.photos.len(), 3);
        assert!(result.photos.iter().all(|p| p.user_id.is_some()));

        // family_only=true → only public photos
        let mut tx = pool.begin().await?;
        let result = tx
            .get_photos_paginated("user1", false, true, None, 10)
            .await?;
        tx.commit().await?;
        assert_eq!(result.photos.len(), 1);
        assert!(result.photos[0].user_id.is_none());

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_folder_photos_paginated(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), Some("vacation"), "v1.jpg"),
            create_test_photo(0, Some("user1"), Some("vacation"), "v2.jpg"),
            create_test_photo(0, None, Some("vacation"), "public_v.jpg"),
            create_test_photo(0, Some("user1"), Some("other"), "o1.jpg"),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        // Existing folder → photos in that folder
        let mut tx = pool.begin().await?;
        let result = tx
            .get_folder_photos_paginated("user1", "vacation", None, 10)
            .await?;
        tx.commit().await?;
        assert_eq!(result.photos.len(), 3);

        // Non-existent folder → empty
        let mut tx = pool.begin().await?;
        let result = tx
            .get_folder_photos_paginated("user1", "nonexistent", None, 10)
            .await?;
        tx.commit().await?;
        assert!(result.photos.is_empty());

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_favorite_photos_paginated(pool: SqlitePool) -> sqlx::Result<()> {
        use crate::repo::FavoritesRepo;

        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), None, "p1.jpg"),
            create_test_photo(0, Some("user1"), None, "p2.jpg"),
            create_test_photo(0, Some("user1"), None, "p3.jpg"),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        // No favorites → empty
        let mut tx = pool.begin().await?;
        let result = tx.get_favorite_photos_paginated("user1", None, 10).await?;
        tx.commit().await?;
        assert!(result.photos.is_empty());

        // Add some favorites
        let ids = pool.get_all_photo_ids().await?;
        pool.insert_favorite(ids[0], "user1").await?;
        pool.insert_favorite(ids[2], "user1").await?;

        // Favorites exist → returned in order
        let mut tx = pool.begin().await?;
        let result = tx.get_favorite_photos_paginated("user1", None, 10).await?;
        tx.commit().await?;
        assert_eq!(result.photos.len(), 2);

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_folders_with_counts(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        // No folders → empty
        let mut tx = pool.begin().await?;
        let folders = tx.get_folders_with_counts("user1", false, false).await?;
        tx.commit().await?;
        assert!(folders.is_empty());

        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), Some("folder_a"), "a1.jpg"),
            create_test_photo(0, Some("user1"), Some("folder_a"), "a2.jpg"),
            create_test_photo(0, Some("user1"), Some("folder_b"), "b1.jpg"),
            create_test_photo(0, Some("user1"), Some(""), "no_folder.jpg"), // empty folder excluded
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        // Multiple folders → all returned
        let mut tx = pool.begin().await?;
        let folders = tx.get_folders_with_counts("user1", false, false).await?;
        tx.commit().await?;
        assert_eq!(folders.len(), 2);

        // photo_count accurate
        let folder_a = folders.iter().find(|f| f.name == "folder_a").unwrap();
        assert_eq!(folder_a.photo_count, 2);

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_month_summaries(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        // No photos → empty
        let mut tx = pool.begin().await?;
        let summaries = tx.get_month_summaries("user1", false, false).await?;
        tx.commit().await?;
        assert!(summaries.is_empty());

        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo_with_time(
                0,
                Some("user1"),
                None,
                "jan1.jpg",
                datetime!(2024-01-15 10:00:00 UTC),
            ),
            create_test_photo_with_time(
                0,
                Some("user1"),
                None,
                "jan2.jpg",
                datetime!(2024-01-20 10:00:00 UTC),
            ),
            create_test_photo_with_time(
                0,
                Some("user1"),
                None,
                "feb1.jpg",
                datetime!(2024-02-10 10:00:00 UTC),
            ),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        // Photos across months → grouped correctly
        let mut tx = pool.begin().await?;
        let summaries = tx.get_month_summaries("user1", false, false).await?;
        tx.commit().await?;
        assert_eq!(summaries.len(), 2);

        // Ordered by year DESC, month DESC
        assert_eq!(summaries[0].year, 2024);
        assert_eq!(summaries[0].month, 2); // February first
        assert_eq!(summaries[0].count, 1);
        assert_eq!(summaries[1].month, 1); // January second
        assert_eq!(summaries[1].count, 2);

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_folder_month_summaries(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        // Non-existent folder → empty
        let mut tx = pool.begin().await?;
        let summaries = tx
            .get_folder_month_summaries("user1", "nonexistent")
            .await?;
        tx.commit().await?;
        assert!(summaries.is_empty());

        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo_with_time(
                0,
                Some("user1"),
                Some("vacation"),
                "v1.jpg",
                datetime!(2024-06-15 10:00:00 UTC),
            ),
            create_test_photo_with_time(
                0,
                Some("user1"),
                Some("vacation"),
                "v2.jpg",
                datetime!(2024-06-20 10:00:00 UTC),
            ),
            create_test_photo_with_time(
                0,
                Some("user1"),
                Some("other"),
                "o1.jpg",
                datetime!(2024-06-15 10:00:00 UTC),
            ),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        // Folder with photos → monthly breakdown
        let mut tx = pool.begin().await?;
        let summaries = tx.get_folder_month_summaries("user1", "vacation").await?;
        tx.commit().await?;
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].count, 2);

        Ok(())
    }
}
