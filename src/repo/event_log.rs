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
    use crate::repo::tests::{create_test_folder, insert_photo, insert_users, photo};
    use crate::repo::{PhotosRepo, PhotosTransactionRepo, UserEventLogError};
    use serde_json::Value;
    use sqlx::SqlitePool;
    use time::OffsetDateTime;

    fn json(photo: &Photo) -> Value {
        serde_json::to_value(photo).unwrap()
    }

    fn parse(data: Option<Vec<u8>>) -> Option<Value> {
        data.map(|data| serde_json::from_slice(&data).unwrap())
    }

    /// Reads both event logs incrementally
    #[derive(Default)]
    struct EventReader {
        photo_cursor: i64,
        folder_cursor: i64,
    }

    impl EventReader {
        /// Asserts the events logged since the last call: `(photo_id, user_id, data)` in
        /// `photos_event_log` and `(folder_id, photo_id, data)` in `folder_event_log`
        async fn expect(
            &mut self,
            pool: &SqlitePool,
            photo_events: &[(i64, Option<&str>, Option<&Photo>)],
            folder_events: &[(i64, i64, Option<&Photo>)],
        ) {
            let rows = query!(
                "select event_id, photo_id, user_id, data from photos_event_log where event_id > $1 order by event_id",
                self.photo_cursor
            )
            .fetch_all(pool)
            .await
            .unwrap();
            self.photo_cursor = rows.last().map_or(self.photo_cursor, |r| r.event_id);
            let actual: Vec<_> = rows
                .into_iter()
                .map(|r| (r.photo_id, r.user_id, parse(r.data)))
                .collect();
            let expected: Vec<_> = photo_events
                .iter()
                .map(|(id, user_id, photo)| (*id, user_id.map(String::from), photo.map(json)))
                .collect();
            assert_eq!(actual, expected, "photos_event_log");

            let rows = query!(
                "select event_id, folder_id, photo_id, data from folder_event_log where event_id > $1 order by event_id",
                self.folder_cursor
            )
            .fetch_all(pool)
            .await
            .unwrap();
            self.folder_cursor = rows.last().map_or(self.folder_cursor, |r| r.event_id);
            let actual: Vec<_> = rows
                .into_iter()
                .map(|r| (r.folder_id, r.photo_id, parse(r.data)))
                .collect();
            let expected: Vec<_> = folder_events
                .iter()
                .map(|(folder_id, id, photo)| (*folder_id, *id, photo.map(json)))
                .collect();
            assert_eq!(actual, expected, "folder_event_log");
        }
    }

    async fn update(pool: &SqlitePool, photo: &Photo) {
        let mut tx = pool.begin().await.unwrap();
        tx.update_photo(photo).await.unwrap();
        tx.commit().await.unwrap();
    }

    /// The logging rules of `CLAUDE.md`, for every mutation of `PhotosTransactionRepo`
    #[sqlx::test]
    async fn test_event_logging_rules(pool: SqlitePool) {
        insert_users(&pool).await;
        let a = create_test_folder(&pool, Some("alice"), "a").await.id;
        let b = create_test_folder(&pool, Some("alice"), "b").await.id;
        let alice = Some("alice");
        let mut log = EventReader::default();

        // Insert outside a folder: only the photo log
        let root = insert_photo(&pool, photo(alice, "root.jpg")).await;
        log.expect(&pool, &[(root.id, alice, Some(&root))], &[])
            .await;

        // Insert into a folder
        let mut p = insert_photo(
            &pool,
            Photo {
                folder_id: Some(a),
                ..photo(alice, "p.jpg")
            },
        )
        .await;
        log.expect(&pool, &[(p.id, alice, Some(&p))], &[(a, p.id, Some(&p))])
            .await;

        // Update inside the folder
        p.name = "renamed.jpg".to_string();
        update(&pool, &p).await;
        log.expect(&pool, &[(p.id, alice, Some(&p))], &[(a, p.id, Some(&p))])
            .await;

        // Move: leaves a, enters b
        p.folder_id = Some(b);
        update(&pool, &p).await;
        log.expect(
            &pool,
            &[(p.id, alice, Some(&p))],
            &[(a, p.id, None), (b, p.id, Some(&p))],
        )
        .await;

        // Trash: leaves b
        p.trashed_on = Some(OffsetDateTime::now_utc());
        update(&pool, &p).await;
        log.expect(&pool, &[(p.id, alice, Some(&p))], &[(b, p.id, None)])
            .await;

        // Updating a trashed photo does not log it into its folder again
        p.name = "trashed.jpg".to_string();
        update(&pool, &p).await;
        log.expect(&pool, &[(p.id, alice, Some(&p))], &[]).await;
        let mut tx = pool.begin().await.unwrap();
        tx.update_thumb_hashes(&[(p.id, vec![1])]).await.unwrap();
        tx.commit().await.unwrap();
        p = pool.get_photo(p.id).await.unwrap().unwrap();
        log.expect(&pool, &[(p.id, alice, Some(&p))], &[]).await;

        // Restore: enters b again
        p.trashed_on = None;
        update(&pool, &p).await;
        log.expect(&pool, &[(p.id, alice, Some(&p))], &[(b, p.id, Some(&p))])
            .await;

        // Made public, out of the folder: public events have no user
        p.user_id = None;
        p.folder_id = None;
        update(&pool, &p).await;
        log.expect(&pool, &[(p.id, None, Some(&p))], &[(b, p.id, None)])
            .await;

        // Permanent deletion
        let q = insert_photo(
            &pool,
            Photo {
                folder_id: Some(a),
                ..photo(alice, "q.jpg")
            },
        )
        .await;
        log.expect(&pool, &[(q.id, alice, Some(&q))], &[(a, q.id, Some(&q))])
            .await;
        let mut tx = pool.begin().await.unwrap();
        tx.delete_photo(&q).await.unwrap();
        tx.commit().await.unwrap();
        log.expect(&pool, &[(q.id, alice, None)], &[(a, q.id, None)])
            .await;

        // Batches
        let mut tx = pool.begin().await.unwrap();
        tx.insert_photos(&[
            photo(alice, "batch_root.jpg"),
            Photo {
                folder_id: Some(a),
                ..photo(alice, "batch_a.jpg")
            },
        ])
        .await
        .unwrap();
        tx.commit().await.unwrap();
        let mut ids = pool.get_all_photo_ids().await.unwrap();
        ids.sort();
        let (x, y) = (ids[ids.len() - 2], ids[ids.len() - 1]);
        let get = async |id| pool.get_photo(id).await.unwrap().unwrap();
        let (px, py) = (get(x).await, get(y).await);
        log.expect(
            &pool,
            &[(x, alice, Some(&px)), (y, alice, Some(&py))],
            &[(a, y, Some(&py))],
        )
        .await;

        let mut tx = pool.begin().await.unwrap();
        tx.update_thumb_hashes(&[(x, vec![1]), (y, vec![2])])
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let (px, py) = (get(x).await, get(y).await);
        log.expect(
            &pool,
            &[(x, alice, Some(&px)), (y, alice, Some(&py))],
            &[(a, y, Some(&py))],
        )
        .await;

        // Batch deletions have no user
        let mut tx = pool.begin().await.unwrap();
        tx.delete_photos(&[x, y]).await.unwrap();
        tx.commit().await.unwrap();
        log.expect(&pool, &[(x, None, None), (y, None, None)], &[(a, y, None)])
            .await;
    }

    #[sqlx::test]
    async fn test_user_events(pool: SqlitePool) -> sqlx::Result<()> {
        insert_users(&pool).await;

        // An empty log is in sync with cursor 0
        let mut tx = pool.begin().await?;
        let events = tx.get_events_for_user(0, "alice").await.unwrap();
        assert_eq!((events.event_log_id, events.events.len()), (0, 0));
        // Any other cursor is outside the (empty) log
        let result = tx.get_events_for_user(5, "alice").await;
        assert!(
            matches!(result, Err(UserEventLogError::InvalidEventId)),
            "{result:?}"
        );
        tx.commit().await?;

        let mine = insert_photo(&pool, photo(Some("alice"), "mine.jpg")).await;
        let mut tx = pool.begin().await?;
        let cursor = tx
            .get_photos_by_user_and_public("alice")
            .await?
            .event_log_id;
        tx.commit().await?;

        let public = insert_photo(&pool, photo(None, "public.jpg")).await;
        insert_photo(&pool, photo(Some("bob"), "bob.jpg")).await;
        let renamed = Photo {
            name: "renamed.jpg".to_string(),
            ..mine.clone()
        };
        update(&pool, &renamed).await;

        let mut tx = pool.begin().await?;
        // Own and public events after the cursor, with the photo JSON in base64
        let events = tx.get_events_for_user(cursor, "alice").await.unwrap();
        let decoded: Vec<_> = events
            .events
            .iter()
            .map(|e| {
                (
                    e.photo_id,
                    e.data.as_ref().map(|d| STANDARD.decode(d).unwrap()),
                )
            })
            .map(|(id, data)| (id, parse(data)))
            .collect();
        assert_eq!(
            decoded,
            [
                (public.id, Some(json(&public))),
                (renamed.id, Some(json(&renamed))),
            ]
        );
        assert_eq!(events.event_log_id, cursor + 3);

        // Up to date
        let events = tx.get_events_for_user(cursor + 3, "alice").await.unwrap();
        assert!(events.events.is_empty());
        assert_eq!(events.event_log_id, cursor + 3);

        // Outside the logged range: the client must do a full sync
        for invalid in [cursor - 100, cursor + 4] {
            let result = tx.get_events_for_user(invalid, "alice").await;
            assert!(
                matches!(result, Err(UserEventLogError::InvalidEventId)),
                "{invalid}"
            );
        }
        tx.commit().await?;

        // Cleaned up events make older cursors stale
        pool.delete_old_events(10).await?;
        let mut tx = pool.begin().await?;
        assert!(tx.get_events_for_user(cursor, "alice").await.is_ok());
        tx.commit().await?;
        pool.delete_old_events(2).await?;
        let mut tx = pool.begin().await?;
        let result = tx.get_events_for_user(cursor, "alice").await;
        assert!(
            matches!(result, Err(UserEventLogError::InvalidEventId)),
            "{result:?}"
        );
        let remaining: Vec<i64> = query_scalar!("select event_id from photos_event_log")
            .fetch_all(tx.as_mut())
            .await?;
        assert_eq!(remaining, [cursor + 2, cursor + 3]);
        // The cursor at the oldest remaining event is still valid
        let events = tx.get_events_for_user(cursor + 2, "alice").await.unwrap();
        assert_eq!((events.event_log_id, events.events.len()), (cursor + 3, 1));
        tx.commit().await?;

        Ok(())
    }

    #[sqlx::test]
    async fn test_folder_events(pool: SqlitePool) -> sqlx::Result<()> {
        insert_users(&pool).await;
        let a = create_test_folder(&pool, Some("alice"), "a").await.id;
        let b = create_test_folder(&pool, Some("alice"), "b").await.id;
        assert_eq!(pool.get_latest_folder_event_id(a).await?, 0);

        let mut photos = Vec::new();
        for (folder_id, name) in [(a, "a1"), (a, "a2"), (a, "a3"), (b, "b1"), (b, "b2")] {
            let photo = Photo {
                folder_id: Some(folder_id),
                ..photo(Some("alice"), name)
            };
            photos.push(insert_photo(&pool, photo).await);
        }
        let latest_a = pool.get_latest_folder_event_id(a).await?;

        // Only the folder's own events after the cursor
        let mut tx = pool.begin().await?;
        let a_ids = folder_event_ids(&pool, a).await?;
        let after_first = tx.get_folder_events(a, a_ids[0]).await?.unwrap();
        let ids: Vec<i64> = after_first.events.iter().map(|e| e.photo_id).collect();
        assert_eq!(ids, [photos[1].id, photos[2].id]);
        assert_eq!(after_first.event_log_id, latest_a);
        let decoded = STANDARD
            .decode(after_first.events[0].data.as_ref().unwrap())
            .unwrap();
        assert_eq!(parse(Some(decoded)), Some(json(&photos[1])));

        let after = tx.get_folder_events(a, latest_a).await?.unwrap();
        assert!(after.events.is_empty());
        assert_eq!(after.event_log_id, latest_a);
        tx.commit().await?;

        // Keeps the latest events of each folder
        pool.delete_old_folder_events(2).await?;
        let remaining: Vec<i64> =
            query_scalar!("select photo_id from folder_event_log order by event_id")
                .fetch_all(&pool)
                .await?;
        assert_eq!(
            remaining,
            photos[1..].iter().map(|p| p.id).collect::<Vec<_>>()
        );

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

    /// The cleanup keeps the last n events of each folder, however far they are behind the
    /// global (cross-folder) event ids
    #[sqlx::test]
    async fn test_delete_old_folder_events_is_per_folder(pool: SqlitePool) -> sqlx::Result<()> {
        insert_users(&pool).await;
        let quiet = create_test_folder(&pool, Some("alice"), "quiet").await.id;
        let busy = create_test_folder(&pool, Some("alice"), "busy").await.id;

        // Interleave: 3 events in the quiet folder among 7 in the busy one
        for i in 0..10 {
            let folder_id = if i % 4 == 0 { quiet } else { busy };
            let photo = Photo {
                folder_id: Some(folder_id),
                ..photo(Some("alice"), &format!("p{i}.jpg"))
            };
            insert_photo(&pool, photo).await;
        }

        let quiet_before = folder_event_ids(&pool, quiet).await?;
        let busy_before = folder_event_ids(&pool, busy).await?;
        assert_eq!((quiet_before.len(), busy_before.len()), (3, 7));

        pool.delete_old_folder_events(3).await?;
        assert_eq!(folder_event_ids(&pool, quiet).await?, quiet_before);
        assert_eq!(folder_event_ids(&pool, busy).await?, &busy_before[4..]);

        pool.delete_old_folder_events(1).await?;
        assert_eq!(folder_event_ids(&pool, quiet).await?, &quiet_before[2..]);
        assert_eq!(folder_event_ids(&pool, busy).await?, &busy_before[6..]);

        Ok(())
    }
}
