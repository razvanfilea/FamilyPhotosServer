use crate::model::photo::Photo;
use sqlx::{QueryBuilder, Sqlite, SqliteExecutor, query};

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
            // An empty vector would cause a SQL syntax error
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

fn photo_to_json_bytes(photo: &Photo) -> sqlx::Result<Vec<u8>> {
    serde_json::to_vec(&photo).map_err(|e| sqlx::Error::Encode(e.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::PhotosTransactionRepo;
    use crate::repo::tests::{create_test_photo, create_test_user, insert_test_user};
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
}
