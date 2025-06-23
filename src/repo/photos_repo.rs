use crate::model::photo::{Photo, PhotoBase, PhotoBody};
use crate::model::user::PUBLIC_USER_ID;
use crate::utils::internal_error;
use axum::response::ErrorResponse;
use sqlx::{QueryBuilder, Sqlite, SqlitePool, query, query_as};

#[derive(Clone)]
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

    pub async fn get_photos_with_same_location(&self) -> Result<Vec<Photo>, ErrorResponse> {
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
        .map_err(internal_error)
    }

    pub async fn insert_photo(&self, photo: &PhotoBody) -> Result<Photo, ErrorResponse> {
        let user_id = photo.user_id();
        let name = photo.name();
        let created_at = photo.created_at();
        let file_size = photo.file_size();
        let folder_name = photo.folder_name();

        query_as!(
            Photo,
            "insert into photos (user_id, name, created_at, file_size, folder) values ($1, $2, $3, $4, $5) returning *",
            user_id,
            name,
            created_at,
            file_size,
            folder_name
        )
        .fetch_one(&self.pool)
        .await
        .map_err(internal_error)
    }

    pub async fn insert_photos(&self, photos: &[PhotoBody]) -> Result<(), sqlx::Error> {
        let mut query_builder: QueryBuilder<Sqlite> =
            QueryBuilder::new("insert into photos (user_id, name, created_at, file_size, folder) ");

        query_builder.push_values(photos, |mut b, photo| {
            b.push_bind(photo.user_id())
                .push_bind(photo.name())
                .push_bind(photo.created_at())
                .push_bind(photo.file_size())
                .push_bind(photo.folder_name());
        });

        query_builder.build().execute(&self.pool).await.map(|_| ())
    }

    pub async fn update_photo(&self, photo: &Photo) -> Result<(), ErrorResponse> {
        let photo_id = photo.id;
        let user_id = photo.user_id();
        let name = photo.name();
        let created_at = photo.created_at();
        let file_size = photo.file_size();
        let folder_name = photo.folder_name();

        query!(
            "update photos set user_id = $2, name = $3, created_at = $4, file_size = $5, folder = $6 where id = $1",
            photo_id,
            user_id,
            name,
            created_at,
            file_size,
            folder_name
        )
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(internal_error)
    }

    pub async fn delete_photo(&self, id: i64) -> Result<u64, ErrorResponse> {
        query!("delete from photos where id = $1", id)
            .execute(&self.pool)
            .await
            .map(|result| result.rows_affected())
            .map_err(internal_error)
    }

    pub async fn delete_photos(&self, photo_ids: &[i64]) -> Result<(), sqlx::Error> {
        if photo_ids.is_empty() {
            return Ok(());
        }

        let mut query_builder: QueryBuilder<Sqlite> =
            QueryBuilder::new("delete from photos where id in (");

        // One element vector is handled correctly, but an empty vector
        // would cause a SQL syntax error
        let mut separated = query_builder.separated(", ");
        for photos in photo_ids.iter() {
            separated.push_bind(photos);
        }
        separated.push_unseparated(") ");

        query_builder.build().execute(&self.pool).await.map(|_| ())
    }
}
