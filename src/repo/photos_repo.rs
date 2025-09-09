use crate::model::event_log::{EventLog, EventLogs};
use crate::model::photo::{FullPhotosList, Photo};
use crate::repo::event_log::EventLogRepo;
use sqlx::{FromRow, QueryBuilder, Sqlite, SqliteExecutor, SqliteTransaction, query, query_as};
use thiserror::Error;

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

    async fn get_all_photos(self) -> sqlx::Result<Vec<Photo>> {
        query_as!(Photo, "select * from photos order by created_at desc")
            .fetch_all(self)
            .await
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
        query!(
            "select id from photos where (($1 is null and user_id is null) or user_id = $1) and folder = $2 order by created_at desc",
            user_id,
            folder_name,
        ).map(|r| r.id).fetch_all(self).await
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
        let lastest_event_id = query!("select max(event_id) as 'event_id' from photos_event_log")
            .map(|r| r.event_id)
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

        let mut all_photos = Vec::with_capacity(photos.len());
        for (id, thumb_hash) in photos.iter() {
            let photo = query_as!(
                Photo,
                "update photos set thumb_hash = $2 where id = $1 returning *",
                id,
                thumb_hash
            )
            .fetch_one(self.as_mut())
            .await?;

            all_photos.push(photo);
        }

        self.insert_creation_event_logs(&all_photos).await
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
