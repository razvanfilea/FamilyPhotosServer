use crate::model::photo::{Photo, PhotoBase, PhotoBody};
use crate::model::user::PUBLIC_USER_ID;
use crate::utils::internal_error;
use axum::response::ErrorResponse;
use sqlx::{FromRow, QueryBuilder, Sqlite, SqlitePool, query, query_as};

pub struct PhotosRepository {
    pool: SqlitePool,
}

impl PhotosRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_photo(&self, id: i64) -> Result<Photo, ErrorResponse> {
        query_as!(Photo, "select * from photos where id = $1", id)
            .fetch_one(&self.pool) // fetch_optional
            .await
            .map_err(internal_error)
    }

    pub async fn get_all_photos(&self) -> Result<Vec<Photo>, ErrorResponse> {
        query_as!(Photo, "select * from photos order by created_at desc")
            .fetch_all(&self.pool)
            .await
            .map_err(internal_error)
    }

    pub async fn get_photos_by_user(
        &self,
        user_id: impl AsRef<str>,
    ) -> Result<Vec<Photo>, ErrorResponse> {
        let user_id = user_id.as_ref();
        query_as!(
            Photo,
            "select * from photos where photos.user_id = $1 order by photos.created_at desc",
            user_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(internal_error)
    }

    pub async fn get_photos_by_user_and_public(
        &self,
        user_id: impl AsRef<str>,
    ) -> Result<Vec<Photo>, ErrorResponse> {
        let user_id = user_id.as_ref();
        query_as!(
            Photo,
            "select * from photos where user_id = $1 or user_id = $2 order by created_at desc",
            user_id,
            PUBLIC_USER_ID
        )
        .fetch_all(&self.pool)
        .await
        .map_err(internal_error)
    }

    pub async fn get_photos_in_folder(
        &self,
        user_id: impl AsRef<str>,
        folder_name: impl AsRef<str>,
    ) -> Result<Vec<Photo>, ErrorResponse> {
        let user_id = user_id.as_ref();
        let folder_name = folder_name.as_ref();

        query_as!(
            Photo,
            "select * from photos where user_id = $1 and folder = $2 order by created_at desc",
            user_id,
            folder_name,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(internal_error)
    }

    pub async fn get_photos_with_same_location(&self) -> Result<Vec<Photo>, sqlx::Error> {
        query_as!(
            Photo,
            "select * from photos
            where rowid not in (
                select min(rowid)
                from photos
                group by user_id, folder, name)",
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn insert_photo(&self, photo: &PhotoBody) -> Result<Photo, sqlx::Error> {
        let user_id = photo.user_id();
        let name = photo.name();
        let created_at = photo.created_at();
        let file_size = photo.file_size();
        let folder_name = photo.folder_name();

        let mut tx = self.pool.begin().await?;

        let photo = query_as!(
            Photo,
            "insert into photos (user_id, name, created_at, file_size, folder) values ($1, $2, $3, $4, $5) returning *",
            user_id,
            name,
            created_at,
            file_size,
            folder_name
        )
            .fetch_one(tx.as_mut())
            .await?;

        let serialized_data = serde_json::to_vec(&photo).expect("Failed to serialize photo");

        query!(
            "insert into photos_event_log (photo_id, user_id, data) values ($1, $2, $3)",
            photo.id,
            photo.user_id,
            serialized_data
        )
        .execute(tx.as_mut())
        .await?;

        tx.commit().await?;

        Ok(photo)
    }

    pub async fn insert_photos(&self, photos: &[PhotoBody]) -> Result<(), sqlx::Error> {
        if photos.is_empty() {
            // One element vector is handled correctly, but an empty vector
            // would cause a SQL syntax error
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;

        let photos = QueryBuilder::<Sqlite>::new(
            "insert into photos (user_id, name, created_at, file_size, folder) ",
        )
        .push_values(photos, |mut b, photo| {
            b.push_bind(photo.user_id())
                .push_bind(photo.name())
                .push_bind(photo.created_at())
                .push_bind(photo.file_size())
                .push_bind(photo.folder_name());
        })
        .push(" returning *")
        .build()
        .try_map(|row| Photo::from_row(&row))
        .fetch_all(tx.as_mut())
        .await?;

        QueryBuilder::<Sqlite>::new("insert into photos_event_log (photo_id, user_id, data) ")
            .push_values(photos, |mut b, photo| {
                let serialized_data =
                    serde_json::to_vec(&photo).expect("Failed to serialize photo");

                b.push_bind(photo.id)
                    .push_bind(photo.user_id)
                    .push_bind(serialized_data);
            })
            .build()
            .execute(tx.as_mut())
            .await?;

        tx.commit().await
    }

    pub async fn update_photo(&self, photo: &Photo) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        query!(
            "update photos set user_id = $2, name = $3, created_at = $4, file_size = $5, folder = $6 where id = $1",
            photo.id,
            photo.user_id,
            photo.name,
            photo.created_at,
            photo.file_size,
            photo.folder
        )
            .execute(tx.as_mut())
            .await?;

        let serialized_data = serde_json::to_vec(&photo).expect("Failed to serialize photo");
        query!(
            "insert into photos_event_log (photo_id, user_id, data) values ($1, $2, $3)",
            photo.id,
            photo.user_id,
            serialized_data
        )
        .execute(tx.as_mut())
        .await?;

        tx.commit().await
    }

    pub async fn delete_photo(&self, photo: &Photo) -> Result<u64, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        let rows_deleted = query!("delete from photos where id = $1", photo.id)
            .execute(tx.as_mut())
            .await
            .map(|result| result.rows_affected())?;

        query!(
            "insert into photos_event_log (photo_id, user_id, data) values ($1, $2, $3)",
            photo.id,
            PUBLIC_USER_ID,
            None::<Vec<u8>>
        )
        .execute(tx.as_mut())
        .await?;

        tx.commit().await?;

        Ok(rows_deleted)
    }

    pub async fn delete_photos(&self, photo_ids: &[i64]) -> Result<(), sqlx::Error> {
        if photo_ids.is_empty() {
            // One element vector is handled correctly, but an empty vector
            // would cause a SQL syntax error
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;

        let mut query_builder: QueryBuilder<Sqlite> =
            QueryBuilder::new("delete from photos where id in (");

        let mut separated = query_builder.separated(", ");
        for photo_id in photo_ids.iter() {
            separated.push_bind(photo_id);
        }
        separated.push_unseparated(") ");

        query_builder.build().execute(tx.as_mut()).await?;

        QueryBuilder::<Sqlite>::new("insert into photos_event_log (photo_id, user_id) ")
            .push_values(photo_ids, |mut b, photo_id| {
                b.push_bind(photo_id).push_bind(PUBLIC_USER_ID);
            })
            .build()
            .execute(tx.as_mut())
            .await?;

        tx.commit().await
    }
}
