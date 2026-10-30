use crate::model::event_log::{EventLog, EventLogs};
use crate::model::photo::Photo;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sqlx::{QueryBuilder, Sqlite, SqliteExecutor, SqliteTransaction, query, query_scalar};

fn photo_to_json_bytes(photo: &Photo) -> sqlx::Result<Vec<u8>> {
    serde_json::to_vec(&photo).map_err(|e| sqlx::Error::Encode(e.into()))
}

pub trait EventLogRepo<'c>: SqliteExecutor<'c> {
    async fn insert_event_log(
        self,
        photo_id: i64,
        user_id: Option<&str>,
        photo: Option<&Photo>,
    ) -> sqlx::Result<()> {
        let serialized_data = match photo {
            Some(photo) => Some(photo_to_json_bytes(photo)?),
            None => None,
        };

        query!(
            "insert into photos_event_log (photo_id, user_id, data) values ($1, $2, $3)",
            photo_id,
            user_id,
            serialized_data
        )
        .execute(self)
        .await
        .map(|_| ())
    }

    async fn insert_creation_event_logs(self, photos: &[Photo]) -> sqlx::Result<()> {
        if photos.is_empty() {
            return Ok(());
        }

        let photos = photos
            .iter()
            .map(|photo| photo_to_json_bytes(photo).map(|data| (photo, data)))
            .collect::<sqlx::Result<Vec<_>>>()?;

        QueryBuilder::<Sqlite>::new("insert into photos_event_log (photo_id, user_id, data) ")
            .push_values(photos, |mut b, (photo, serialized_data)| {
                b.push_bind(photo.id)
                    .push_bind(&photo.user_id)
                    .push_bind(serialized_data);
            })
            .build()
            .execute(self)
            .await
            .map(|_| ())
    }

    async fn insert_deletion_event_logs(self, photo_ids: &[i64]) -> sqlx::Result<()> {
        QueryBuilder::<Sqlite>::new("insert into photos_event_log (photo_id) ")
            .push_values(photo_ids, |mut b, photo_id| {
                b.push_bind(photo_id);
            })
            .build()
            .execute(self)
            .await
            .map(|_| ())
    }

    async fn delete_old_events(self, last_rows_to_keep: u32) -> Result<(), sqlx::Error> {
        query!("delete from photos_event_log where event_id <= (select max(event_id) from photos_event_log) - $1", last_rows_to_keep)
            .execute(self)
            .await
            .map(|_| {})
    }
}

impl<'c, E> EventLogRepo<'c> for E where E: SqliteExecutor<'c> {}

pub trait FolderEventLogRepo<'c>: SqliteExecutor<'c> {
    async fn get_latest_folder_event_id(self, folder_id: i64) -> sqlx::Result<i64> {
        query_scalar!(
            "select coalesce(max(event_id), 0) as 'id!: i64' from folder_event_log where folder_id = $1",
            folder_id
        )
        .fetch_one(self)
        .await
    }

    async fn insert_folder_event(
        self,
        folder_id: i64,
        photo_id: i64,
        photo: Option<&Photo>,
    ) -> sqlx::Result<()> {
        let data = match photo {
            Some(photo) => Some(photo_to_json_bytes(photo)?),
            None => None,
        };

        query!(
            "insert into folder_event_log (folder_id, photo_id, data) values ($1, $2, $3)",
            folder_id,
            photo_id,
            data
        )
        .execute(self)
        .await
        .map(|_| ())
    }

    async fn insert_folder_creation_event_logs(self, photos: &[Photo]) -> sqlx::Result<()> {
        let photos_with_folders: Vec<_> = photos
            .iter()
            .filter(|p| p.trashed_on.is_none())
            .filter_map(|p| p.folder_id.map(|fid| (fid, p)))
            .collect();

        if photos_with_folders.is_empty() {
            return Ok(());
        }

        let entries = photos_with_folders
            .iter()
            .map(|(fid, photo)| photo_to_json_bytes(photo).map(|data| (*fid, photo.id, data)))
            .collect::<sqlx::Result<Vec<_>>>()?;

        QueryBuilder::<Sqlite>::new("insert into folder_event_log (folder_id, photo_id, data) ")
            .push_values(entries, |mut b, (folder_id, photo_id, data)| {
                b.push_bind(folder_id).push_bind(photo_id).push_bind(data);
            })
            .build()
            .execute(self)
            .await
            .map(|_| ())
    }

    async fn insert_folder_deletion_event_logs(self, entries: &[(i64, i64)]) -> sqlx::Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        QueryBuilder::<Sqlite>::new("insert into folder_event_log (folder_id, photo_id) ")
            .push_values(entries, |mut b, (folder_id, photo_id)| {
                b.push_bind(folder_id).push_bind(photo_id);
            })
            .build()
            .execute(self)
            .await
            .map(|_| ())
    }

    /// Keeps only the last `max_per_folder` events of each folder. Event ids are global
    /// across folders, so the cutoff has to be computed per folder. This always deletes
    /// a prefix (the oldest events) of each folder's log.
    async fn delete_old_folder_events(self, max_per_folder: u32) -> sqlx::Result<()> {
        query!(
            r#"delete from folder_event_log where event_id in (
                select event_id from (
                    select event_id,
                        row_number() over (partition by folder_id order by event_id desc) as rn
                    from folder_event_log
                )
                where rn > $1
            )"#,
            max_per_folder
        )
        .execute(self)
        .await
        .map(|_| ())
    }
}

impl<'c, E> FolderEventLogRepo<'c> for E where E: SqliteExecutor<'c> {}

pub trait FolderEventLogTransactionRepo {
    /// Returns the events of the folder after `after_event_id` (> 0), or `None` if the
    /// cursor is stale (see `sync_folders`).
    async fn get_folder_events(
        &mut self,
        folder_id: i64,
        after_event_id: i64,
    ) -> sqlx::Result<Option<EventLogs>>;
}

impl FolderEventLogTransactionRepo for SqliteTransaction<'_> {
    async fn get_folder_events(
        &mut self,
        folder_id: i64,
        after_event_id: i64,
    ) -> sqlx::Result<Option<EventLogs>> {
        let (min_event_id, max_event_id) = query!(
            "select min(event_id) as 'min_id: i64', max(event_id) as 'max_id: i64' from folder_event_log where folder_id = $1",
            folder_id
        )
        .map(|record| (record.min_id, record.max_id))
        .fetch_one(self.as_mut())
        .await?;

        let (Some(min_event_id), Some(max_event_id)) = (min_event_id, max_event_id) else {
            return Ok(None);
        };

        if min_event_id > after_event_id || max_event_id < after_event_id {
            return Ok(None);
        }

        let events = query!(
            "select photo_id, data from folder_event_log where folder_id = $1 and event_id > $2 order by event_id",
            folder_id,
            after_event_id,
        )
        .map(|record| EventLog {
            photo_id: record.photo_id,
            data: record.data.map(|bytes| STANDARD.encode(bytes)),
        })
        .fetch_all(self.as_mut())
        .await?;

        Ok(Some(EventLogs {
            event_log_id: max_event_id,
            events,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::PhotosTransactionRepo;
    use crate::repo::tests::{
        create_test_folder, create_test_photo, create_test_user, insert_test_user,
    };
    use sqlx::SqlitePool;

    #[sqlx::test]
    async fn test_insert_event_log(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let mut tx = pool.begin().await?;
        let photo = create_test_photo(0, Some("user1"), None, "test.jpg");
        let inserted = tx.insert_photo(&photo).await?;
        tx.commit().await?;

        // Verify event was created with data
        let event = sqlx::query!(
            "select event_id, photo_id, user_id, data from photos_event_log where photo_id = $1",
            inserted.id
        )
        .fetch_one(&pool)
        .await?;

        assert_eq!(event.photo_id, inserted.id);
        assert_eq!(event.user_id, Some("user1".to_string()));
        assert!(event.data.is_some()); // data contains JSON

        // Insert deletion event (no photo data)
        pool.insert_event_log(inserted.id, Some("user1"), None)
            .await?;

        let events = sqlx::query!(
            "select event_id, data from photos_event_log where photo_id = $1 order by event_id",
            inserted.id
        )
        .fetch_all(&pool)
        .await?;

        assert_eq!(events.len(), 2);
        assert!(events[0].data.is_some()); // creation event has data
        assert!(events[1].data.is_none()); // deletion event has NULL data

        // Public photo → user_id is NULL in event
        let mut tx = pool.begin().await?;
        let public_photo = create_test_photo(0, None, None, "public.jpg");
        let public_inserted = tx.insert_photo(&public_photo).await?;
        tx.commit().await?;

        let event = sqlx::query!(
            "select user_id from photos_event_log where photo_id = $1",
            public_inserted.id
        )
        .fetch_one(&pool)
        .await?;

        assert!(event.user_id.is_none());

        Ok(())
    }

    #[sqlx::test]
    async fn test_batch_event_logs(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        // insert_creation_event_logs empty → Ok
        pool.insert_creation_event_logs(&[]).await?;

        // Count events
        let count: i32 =
            sqlx::query_scalar!("select count(*) as 'count!: i32' from photos_event_log")
                .fetch_one(&pool)
                .await?;
        assert_eq!(count, 0);

        // Insert photos (which creates events)
        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), None, "p1.jpg"),
            create_test_photo(0, Some("user1"), None, "p2.jpg"),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        let count: i32 =
            sqlx::query_scalar!("select count(*) as 'count!: i32' from photos_event_log")
                .fetch_one(&pool)
                .await?;
        assert_eq!(count, 2);

        // insert_deletion_event_logs → NULL data for each
        let ids: Vec<i64> = sqlx::query_scalar!("select id from photos")
            .fetch_all(&pool)
            .await?;

        pool.insert_deletion_event_logs(&ids).await?;

        let deletion_events = sqlx::query!("select data from photos_event_log where data is null")
            .fetch_all(&pool)
            .await?;

        assert_eq!(deletion_events.len(), 2);

        Ok(())
    }

    #[sqlx::test]
    async fn test_delete_old_events(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        // Empty table → Ok
        pool.delete_old_events(10).await?;

        // Insert some photos to create events
        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), None, "p1.jpg"),
            create_test_photo(0, Some("user1"), None, "p2.jpg"),
            create_test_photo(0, Some("user1"), None, "p3.jpg"),
            create_test_photo(0, Some("user1"), None, "p4.jpg"),
            create_test_photo(0, Some("user1"), None, "p5.jpg"),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        let count: i32 =
            sqlx::query_scalar!("select count(*) as 'count!: i32' from photos_event_log")
                .fetch_one(&pool)
                .await?;
        assert_eq!(count, 5);

        // last_rows_to_keep >= total → nothing deleted
        pool.delete_old_events(10).await?;

        let count: i32 =
            sqlx::query_scalar!("select count(*) as 'count!: i32' from photos_event_log")
                .fetch_one(&pool)
                .await?;
        assert_eq!(count, 5);

        // last_rows_to_keep < total → old events deleted
        pool.delete_old_events(2).await?;

        let count: i32 =
            sqlx::query_scalar!("select count(*) as 'count!: i32' from photos_event_log")
                .fetch_one(&pool)
                .await?;
        assert_eq!(count, 2);

        // Verify the kept events are the most recent
        let max_id: i64 =
            sqlx::query_scalar!("select max(event_id) as 'max!: i64' from photos_event_log")
                .fetch_one(&pool)
                .await?;

        let min_id: i64 =
            sqlx::query_scalar!("select min(event_id) as 'min!: i64' from photos_event_log")
                .fetch_one(&pool)
                .await?;

        // The two remaining should be consecutive and at the end
        assert_eq!(max_id - min_id, 1);

        Ok(())
    }

    #[sqlx::test]
    async fn test_insert_folder_event(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let folder = create_test_folder(&pool, Some("user1"), "vacation").await;

        let mut tx = pool.begin().await?;
        let photo = create_test_photo(0, Some("user1"), Some(folder.id), "test.jpg");
        let inserted = tx.insert_photo(&photo).await?;
        tx.commit().await?;

        // Verify folder event was created with data
        let event = sqlx::query!(
            "select event_id, folder_id, photo_id, data from folder_event_log where photo_id = $1",
            inserted.id
        )
        .fetch_one(&pool)
        .await?;

        assert_eq!(event.folder_id, folder.id);
        assert_eq!(event.photo_id, inserted.id);
        assert!(event.data.is_some());

        Ok(())
    }

    #[sqlx::test]
    async fn test_no_folder_event_without_folder(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let mut tx = pool.begin().await?;
        let photo = create_test_photo(0, Some("user1"), None, "test.jpg");
        tx.insert_photo(&photo).await?;
        tx.commit().await?;

        let count: i32 =
            sqlx::query_scalar!("select count(*) as 'count!: i32' from folder_event_log")
                .fetch_one(&pool)
                .await?;
        assert_eq!(count, 0);

        Ok(())
    }

    #[sqlx::test]
    async fn test_deletion_folder_event(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let folder = create_test_folder(&pool, Some("user1"), "vacation").await;

        pool.insert_folder_event(folder.id, 1, None).await?;

        let event = sqlx::query!(
            "select data from folder_event_log where folder_id = $1",
            folder.id
        )
        .fetch_one(&pool)
        .await?;

        assert!(event.data.is_none());

        Ok(())
    }

    #[sqlx::test]
    async fn test_delete_old_folder_events(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let folder = create_test_folder(&pool, Some("user1"), "vacation").await;

        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), Some(folder.id), "p1.jpg"),
            create_test_photo(0, Some("user1"), Some(folder.id), "p2.jpg"),
            create_test_photo(0, Some("user1"), Some(folder.id), "p3.jpg"),
            create_test_photo(0, Some("user1"), Some(folder.id), "p4.jpg"),
            create_test_photo(0, Some("user1"), Some(folder.id), "p5.jpg"),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        let count: i32 =
            sqlx::query_scalar!("select count(*) as 'count!: i32' from folder_event_log")
                .fetch_one(&pool)
                .await?;
        assert_eq!(count, 5);

        pool.delete_old_folder_events(2).await?;

        let count: i32 =
            sqlx::query_scalar!("select count(*) as 'count!: i32' from folder_event_log")
                .fetch_one(&pool)
                .await?;
        assert_eq!(count, 2);

        Ok(())
    }

    async fn folder_event_ids(pool: &SqlitePool, folder_id: i64) -> sqlx::Result<Vec<i64>> {
        sqlx::query_scalar!(
            "select event_id from folder_event_log where folder_id = $1 order by event_id",
            folder_id
        )
        .fetch_all(pool)
        .await
    }

    #[sqlx::test]
    async fn test_delete_old_folder_events_is_per_folder(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let quiet = create_test_folder(&pool, Some("user1"), "quiet").await;
        let busy = create_test_folder(&pool, Some("user1"), "busy").await;

        // Interleave: 3 events in the quiet folder among 10 in the busy one
        for i in 0..10 {
            let folder_id = if i % 4 == 0 { quiet.id } else { busy.id };
            let mut tx = pool.begin().await?;
            tx.insert_photo(&create_test_photo(
                0,
                Some("user1"),
                Some(folder_id),
                &format!("p{i}.jpg"),
            ))
            .await?;
            tx.commit().await?;
        }

        let quiet_before = folder_event_ids(&pool, quiet.id).await?;
        let busy_before = folder_event_ids(&pool, busy.id).await?;
        assert_eq!((quiet_before.len(), busy_before.len()), (3, 7));

        pool.delete_old_folder_events(3).await?;

        assert_eq!(folder_event_ids(&pool, quiet.id).await?, quiet_before);
        assert_eq!(folder_event_ids(&pool, busy.id).await?, &busy_before[4..]);

        pool.delete_old_folder_events(1).await?;
        assert_eq!(folder_event_ids(&pool, quiet.id).await?, &quiet_before[2..]);
        assert_eq!(folder_event_ids(&pool, busy.id).await?, &busy_before[6..]);

        Ok(())
    }
}
