use crate::model::event_log::{EventLog, EventLogs};
use crate::model::photo::Photo;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sqlx::{QueryBuilder, Sqlite, SqliteExecutor, SqliteTransaction, query, query_scalar};

pub trait FolderEventLogRepo<'c>: SqliteExecutor<'c> {
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

    async fn get_latest_folder_event_id(self, folder_id: i64) -> sqlx::Result<i64> {
        query_scalar!(
            "select coalesce(max(event_id), 0) as 'id!: i64' from folder_event_log where folder_id = $1",
            folder_id
        )
        .fetch_one(self)
        .await
    }

    async fn delete_old_folder_events(self, max_per_folder: u32) -> sqlx::Result<()> {
        query!(
            r#"delete from folder_event_log where event_id in (
                select e.event_id from folder_event_log e
                inner join (
                    select folder_id, max(event_id) - $1 as cutoff
                    from folder_event_log
                    group by folder_id
                ) c on e.folder_id = c.folder_id and e.event_id <= c.cutoff
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
    async fn get_folder_events(
        &mut self,
        folder_id: i64,
        after_event_id: i64,
    ) -> sqlx::Result<EventLogs>;
}

impl FolderEventLogTransactionRepo for SqliteTransaction<'_> {
    async fn get_folder_events(
        &mut self,
        folder_id: i64,
        after_event_id: i64,
    ) -> sqlx::Result<EventLogs> {
        let max_event_id = query_scalar!(
            "select coalesce(max(event_id), 0) as 'id!: i64' from folder_event_log where folder_id = $1",
            folder_id
        )
        .fetch_one(self.as_mut())
        .await?;

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

        Ok(EventLogs {
            event_log_id: max_event_id,
            events,
        })
    }
}

fn photo_to_json_bytes(photo: &Photo) -> sqlx::Result<Vec<u8>> {
    serde_json::to_vec(&photo).map_err(|e| sqlx::Error::Encode(e.into()))
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
}
