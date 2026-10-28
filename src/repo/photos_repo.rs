use super::{EventLogRepo, FolderEventLogRepo};
use crate::model::event_log::{EventLog, EventLogs};
use crate::model::photo::{FullPhotosList, Photo, PhotoWithFolder};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sqlx::{
    FromRow, QueryBuilder, Sqlite, SqliteExecutor, SqliteTransaction, query, query_as, query_scalar,
};
use thiserror::Error;
use time::OffsetDateTime;

pub use crate::model::photo_access::PhotoAccess;

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
    async fn get_accessible_photo(
        self,
        id: i64,
        user_id: &str,
        access: PhotoAccess,
    ) -> sqlx::Result<Option<Photo>> {
        let access_level = access as i32;
        query_as!(
            Photo,
            r#"select p.* from photos p
            left join folder_permissions fp
                on fp.folder_id = p.folder_id and fp.grantee_id = $2
            where p.id = $1 and (
                p.user_id is null
                or p.user_id = $2
                or (
                    fp.grantee_id is not null
                    and (fp.expires_at is null or datetime(fp.expires_at) > datetime('now'))
                    and ($3 = 1 or ($3 = 2 and fp.can_delete = true))
                )
            )"#,
            id,
            user_id,
            access_level,
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
        query_as!(Photo, "select * from photos where trashed_on is not null and datetime(trashed_on) <= datetime('now', '-30 days')")
            .fetch_all(self)
            .await
    }

    /// Returns up to `limit` photos without a thumb hash and with an id greater than `last_id`, ordered by id.
    async fn get_photos_without_thumb_hash(
        self,
        last_id: i64,
        limit: i64,
    ) -> sqlx::Result<Vec<Photo>> {
        query_as!(
            Photo,
            "select * from photos where thumb_hash is null and id > $1 order by id limit $2",
            last_id,
            limit
        )
        .fetch_all(self)
        .await
    }

    async fn get_accessible_photo_with_folder(
        self,
        id: i64,
        user_id: &str,
        access: PhotoAccess,
    ) -> sqlx::Result<Option<PhotoWithFolder>> {
        let access_level = access as i32;
        query_as!(
            PhotoWithFolderRow,
            r#"select p.*, f.name as "folder_name: String"
            from photos p
            left join folders f on f.id = p.folder_id
            left join folder_permissions fp
                on fp.folder_id = p.folder_id and fp.grantee_id = $2
            where p.id = $1 and (
                p.user_id is null
                or p.user_id = $2
                or (
                    fp.grantee_id is not null
                    and (fp.expires_at is null or datetime(fp.expires_at) > datetime('now'))
                    and ($3 = 1 or ($3 = 2 and fp.can_delete = true))
                )
            )"#,
            id,
            user_id,
            access_level,
        )
        .fetch_optional(self)
        .await
        .map(|opt| opt.map(PhotoWithFolderRow::into_photo_with_folder))
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
        let (min_event_id, max_event_id) = query!(
            "select coalesce(min(event_id), 0) as 'min_id!: i64', coalesce(max(event_id), 0) as 'max_id!: i64' from photos_event_log",
        ).map(|record| (record.min_id, record.max_id))
            .fetch_one(self.as_mut()).await?;

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
            "insert into photos (user_id, name, created_at, file_size, folder_id, trashed_on) values ($1, $2, $3, $4, $5, $6) returning id, user_id as \"user_id?\", name, created_at, file_size, folder_id as \"folder_id?\", thumb_hash, trashed_on",
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

        if let Some(folder_id) = photo.folder_id {
            self.as_mut()
                .insert_folder_event(folder_id, photo.id, Some(&photo))
                .await?;
        }

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

        self.insert_creation_event_logs(&photos).await?;
        self.as_mut()
            .insert_folder_creation_event_logs(&photos)
            .await
    }

    /// Thumb hash is purposely left out, as [`Self::update_thumb_hashes`] exists
    async fn update_photo(&mut self, photo: &Photo) -> sqlx::Result<()> {
        let old = query_as!(Photo, "select * from photos where id = $1", photo.id)
            .fetch_one(self.as_mut())
            .await?;

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
            .await?;

        let left_old_folder = old.folder_id.is_some()
            && (old.folder_id != photo.folder_id
                || (old.trashed_on.is_none() && photo.trashed_on.is_some()));

        let entered_new_folder = photo.folder_id.is_some()
            && photo.trashed_on.is_none()
            && (old.folder_id != photo.folder_id
                || (old.trashed_on.is_some() && photo.trashed_on.is_none()));

        if left_old_folder && let Some(old_folder_id) = old.folder_id {
            self.as_mut()
                .insert_folder_event(old_folder_id, photo.id, None)
                .await?;
        }

        if entered_new_folder && let Some(new_folder_id) = photo.folder_id {
            self.as_mut()
                .insert_folder_event(new_folder_id, photo.id, Some(photo))
                .await?;
        } else if !left_old_folder
            && photo.trashed_on.is_none()
            && let Some(folder_id) = photo.folder_id
        {
            self.as_mut()
                .insert_folder_event(folder_id, photo.id, Some(photo))
                .await?;
        }

        Ok(())
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

        self.insert_creation_event_logs(&updated_photos).await?;
        self.as_mut()
            .insert_folder_creation_event_logs(&updated_photos)
            .await
    }

    async fn delete_photo(&mut self, photo: &Photo) -> sqlx::Result<u64> {
        query!("delete from favorite_photos where photo_id = $1", photo.id)
            .execute(self.as_mut())
            .await?;

        let rows_deleted = query!("delete from photos where id = $1", photo.id)
            .execute(self.as_mut())
            .await
            .map(|result| result.rows_affected())?;

        self.insert_event_log(photo.id, photo.user_id.as_deref(), None)
            .await?;

        if let Some(folder_id) = photo.folder_id {
            self.as_mut()
                .insert_folder_event(folder_id, photo.id, None)
                .await?;
        }

        Ok(rows_deleted)
    }

    async fn delete_photos(&mut self, photo_ids: &[i64]) -> sqlx::Result<u64> {
        if photo_ids.is_empty() {
            return Ok(0);
        }

        // Fetch folder_ids before deletion for folder_event_log
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "select id, folder_id from photos where folder_id is not null and id in (",
        );
        let mut sep = qb.separated(", ");
        for id in photo_ids {
            sep.push_bind(*id);
        }
        sep.push_unseparated(")");
        let folder_entries: Vec<(i64, i64)> = qb
            .build()
            .try_map(|row: sqlx::sqlite::SqliteRow| {
                use sqlx::Row;
                Ok((row.get::<i64, _>("folder_id"), row.get::<i64, _>("id")))
            })
            .fetch_all(self.as_mut())
            .await?;

        let mut fav_qb: QueryBuilder<Sqlite> =
            QueryBuilder::new("delete from favorite_photos where photo_id in (");
        let mut sep2 = fav_qb.separated(", ");
        for photo_id in photo_ids.iter() {
            sep2.push_bind(photo_id);
        }
        sep2.push_unseparated(")");
        fav_qb.build().execute(self.as_mut()).await?;

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

        if !folder_entries.is_empty() {
            self.as_mut()
                .insert_folder_deletion_event_logs(&folder_entries)
                .await?;
        }

        Ok(rows_deleted)
    }
}

#[derive(Debug, Error)]
pub enum UserEventLogError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("Invalid event id parameter")]
    InvalidEventId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::FavoritesRepo;
    use crate::repo::tests::{
        Dataset, create_test_folder, earlier_same_utc_day, insert_photo, insert_users, photo,
    };
    use sqlx::SqlitePool;
    use std::collections::HashSet;
    use time::Duration;

    #[sqlx::test]
    async fn test_photo_access_matrix(pool: SqlitePool) -> sqlx::Result<()> {
        insert_users(&pool).await;
        let data = Dataset::create(&pool).await;

        // (photo, [Own, Read, Delete]) from alice's point of view. Trashed photos stay accessible.
        #[rustfmt::skip]
        let cases = [
            ("own_1", [true, true, true]),
            ("own_2", [true, true, true]),
            ("own_trashed", [true, true, true]),
            ("public_a", [true, true, true]),
            ("public_trashed", [true, true, true]),
            ("shared_1", [false, true, false]),
            ("shared_trashed", [false, true, false]),
            ("shared_delete_1", [false, true, true]),
            ("expired_1", [false, false, false]),
            ("unshared_1", [false, false, false]),
            ("bob_root", [false, false, false]),
            ("carol_root", [false, false, false]),
        ];
        let levels = [PhotoAccess::Own, PhotoAccess::Read, PhotoAccess::Delete];
        for (name, expected) in cases {
            let id = data.id(name);
            for (access, expected) in levels.into_iter().zip(expected) {
                let photo = pool.get_accessible_photo(id, "alice", access).await?;
                assert_eq!(
                    photo.map(|p| p.id),
                    expected.then_some(id),
                    "{name} {access:?}"
                );

                let with_folder = pool
                    .get_accessible_photo_with_folder(id, "alice", access)
                    .await?;
                assert_eq!(with_folder.is_some(), expected, "{name} {access:?}");
            }
        }

        // The owner has full access to their shared photos
        let shared_1 = data.id("shared_1");
        let photo = pool
            .get_accessible_photo_with_folder(shared_1, "bob", PhotoAccess::Own)
            .await?
            .unwrap();
        assert_eq!(photo.folder_name.as_deref(), Some("shared"));
        assert_eq!(photo.partial_path(), "bob/shared/shared_1");

        let missing = pool.get_accessible_photo(999, "alice", PhotoAccess::Own);
        assert!(missing.await?.is_none());
        // `get_photo` doesn't check access
        let carol_root = pool.get_photo(data.id("carol_root")).await?.unwrap();
        assert_eq!(carol_root.name, "carol_root");
        assert!(pool.get_photo(999).await?.is_none());

        Ok(())
    }

    #[sqlx::test]
    async fn test_photo_lists(pool: SqlitePool) -> sqlx::Result<()> {
        insert_users(&pool).await;

        let mut tx = pool.begin().await?;
        let empty = tx.get_photos_by_user_and_public("alice").await?;
        tx.commit().await?;
        assert_eq!(empty.event_log_id, 0);
        assert!(empty.photos.is_empty());
        assert!(pool.get_all_photo_ids().await?.is_empty());
        assert!(pool.get_expired_trash_photos().await?.is_empty());

        let data = Dataset::create(&pool).await;
        let names = |photos: &[Photo]| data.names(photos.iter().map(|p| p.id));
        let name_set = |photos: &[Photo]| names(photos).into_iter().collect::<HashSet<_>>();

        let mut tx = pool.begin().await?;
        let full = tx.get_photos_by_user_and_public("alice").await?;
        tx.commit().await?;
        let latest_event: i64 = sqlx::query_scalar("select max(event_id) from photos_event_log")
            .fetch_one(&pool)
            .await?;
        assert_eq!(full.event_log_id, latest_event);
        // Own and public photos, trashed included, no shared ones
        assert_eq!(
            name_set(&full.photos),
            HashSet::from([
                "own_1",
                "own_2",
                "own_old",
                "own_trashed",
                "own_big",
                "public_a",
                "public_b",
                "public_trashed",
                "public_big",
            ])
        );

        assert_eq!(pool.get_all_photo_ids().await?.len(), 17);
        // Newest first
        assert_eq!(
            names(&pool.get_photos_by_user(Some("alice")).await?),
            ["own_1", "own_trashed", "own_2", "own_big", "own_old"]
        );
        assert_eq!(
            name_set(&pool.get_photos_by_user(None).await?),
            HashSet::from(["public_a", "public_b", "public_trashed", "public_big"])
        );
        assert_eq!(
            names(&pool.get_photos_in_folder(data.shared_folder).await?),
            ["shared_1"]
        );

        // Trashed 15 days ago: not expired yet
        let recent = Photo {
            trashed_on: Some(OffsetDateTime::now_utc() - Duration::days(15)),
            ..photo(Some("alice"), "recent")
        };
        insert_photo(&pool, recent).await;
        // Trashed a few hours more than 30 days ago, on the same UTC day as the cutoff: expired
        let cutoff = OffsetDateTime::now_utc() - Duration::days(30);
        let same_day = Photo {
            trashed_on: Some(earlier_same_utc_day(cutoff, Duration::hours(3))),
            ..photo(Some("alice"), "same_day")
        };
        let same_day = insert_photo(&pool, same_day).await.id;
        let mut expired = pool.get_expired_trash_photos().await?;
        let before = expired.len();
        expired.retain(|p| p.id != same_day);
        assert_eq!(expired.len(), before - 1, "same_day is expired");
        assert_eq!(
            name_set(&expired),
            HashSet::from([
                "own_trashed",
                "public_trashed",
                "shared_trashed",
                "shared_delete_trashed",
            ])
        );

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_photos_with_same_location(pool: SqlitePool) -> sqlx::Result<()> {
        insert_users(&pool).await;
        let folder = create_test_folder(&pool, Some("alice"), "folder").await;
        let bobs_folder = create_test_folder(&pool, Some("bob"), "folder").await;
        let in_folder = |user_id, folder_id, name| Photo {
            folder_id: Some(folder_id),
            ..photo(user_id, name)
        };

        insert_photo(&pool, in_folder(Some("alice"), folder.id, "a.jpg")).await;
        insert_photo(&pool, in_folder(Some("alice"), folder.id, "b.jpg")).await;
        // Same name, but another user's folder
        insert_photo(&pool, in_folder(Some("bob"), bobs_folder.id, "a.jpg")).await;
        assert!(pool.get_photos_with_same_location().await?.is_empty());

        // All but the first photo with the same user, folder and name
        let duplicate = insert_photo(&pool, in_folder(Some("alice"), folder.id, "a.jpg")).await;
        assert_eq!(pool.get_photos_with_same_location().await?, [duplicate]);

        Ok(())
    }

    #[sqlx::test]
    async fn test_insert_update_delete(pool: SqlitePool) -> sqlx::Result<()> {
        insert_users(&pool).await;
        let folder = create_test_folder(&pool, Some("alice"), "original").await;
        let other_folder = create_test_folder(&pool, Some("alice"), "other").await;

        let public = insert_photo(&pool, photo(None, "public.jpg")).await;
        assert!(public.id > 0);
        assert_eq!(public.user_id, None);

        let inserted = insert_photo(
            &pool,
            Photo {
                folder_id: Some(folder.id),
                ..photo(Some("alice"), "a.jpg")
            },
        )
        .await;
        assert_eq!(inserted.user_id.as_deref(), Some("alice"));
        assert_eq!(pool.get_photo(inserted.id).await?, Some(inserted.clone()));

        // Every column but the thumb hash is updated
        let updated = Photo {
            name: "renamed.jpg".to_string(),
            folder_id: Some(other_folder.id),
            created_at: OffsetDateTime::UNIX_EPOCH,
            file_size: 5,
            trashed_on: Some(OffsetDateTime::UNIX_EPOCH),
            thumb_hash: Some(vec![1]),
            ..inserted.clone()
        };
        let mut tx = pool.begin().await?;
        tx.update_photo(&updated).await?;
        tx.commit().await?;
        assert_eq!(
            pool.get_photo(inserted.id).await?,
            Some(Photo {
                thumb_hash: None,
                ..updated.clone()
            })
        );

        // Batches: empty ones are no-ops, `insert_photos` also stores the thumb hash
        let mut tx = pool.begin().await?;
        tx.insert_photos(&[]).await?;
        tx.update_thumb_hashes(&[]).await?;
        assert_eq!(tx.delete_photos(&[]).await?, 0);
        tx.insert_photos(&[
            Photo {
                thumb_hash: Some(vec![9]),
                ..photo(Some("alice"), "hashed.jpg")
            },
            photo(Some("alice"), "b.jpg"),
        ])
        .await?;
        tx.commit().await?;

        let without_hash = pool.get_photos_without_thumb_hash(0, 100).await?;
        let without_hash: Vec<_> = without_hash.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(without_hash, ["public.jpg", "renamed.jpg", "b.jpg"]);

        let mut tx = pool.begin().await?;
        tx.update_thumb_hashes(&[(public.id, vec![1, 2]), (inserted.id, vec![3, 4])])
            .await?;
        tx.commit().await?;
        let thumb_hash = |id| pool.get_photo(id);
        assert_eq!(
            thumb_hash(public.id).await?.unwrap().thumb_hash,
            Some(vec![1, 2])
        );
        assert_eq!(
            thumb_hash(inserted.id).await?.unwrap().thumb_hash,
            Some(vec![3, 4])
        );

        // Deleting also removes the favourites
        pool.insert_favorite(inserted.id, "alice").await?;
        let mut tx = pool.begin().await?;
        assert_eq!(tx.delete_photo(&updated).await?, 1);
        tx.commit().await?;
        assert!(pool.get_photo(inserted.id).await?.is_none());
        assert!(pool.get_favorite_photos("alice").await?.is_empty());

        let ids = pool.get_all_photo_ids().await?;
        assert_eq!(ids.len(), 3);
        let mut tx = pool.begin().await?;
        // Only existing photos count
        assert_eq!(tx.delete_photos(&[ids[0], ids[1], 9999]).await?, 2);
        tx.commit().await?;
        assert_eq!(pool.get_all_photo_ids().await?, [ids[2]]);

        Ok(())
    }
}
