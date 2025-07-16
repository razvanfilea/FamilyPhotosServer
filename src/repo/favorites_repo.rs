use axum::response::ErrorResponse;
use sqlx::{query, SqlitePool};
use crate::utils::internal_error;

pub struct FavoritesRepository {
    pool: SqlitePool,
}

impl FavoritesRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_favorite_photos(
        &self,
        user_id: impl AsRef<str>,
    ) -> Result<Vec<i64>, ErrorResponse> {
        let user_id = user_id.as_ref();
        query!(
            "select photo_id from favorite_photos where user_id = $1",
            user_id
        )
            .fetch_all(&self.pool)
            .await
            .map(|list| list.into_iter().map(|record| record.photo_id).collect())
            .map_err(internal_error)
    }
    
    pub async fn insert_favorite<T: AsRef<str>>(
        &self,
        photo_id: i64,
        user_id: T,
    ) -> Result<(), ErrorResponse> {
        let user_id = user_id.as_ref();
        query!(
            "insert into favorite_photos (photo_id, user_id) values ($1, $2)",
            photo_id,
            user_id
        )
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(internal_error)
    }

    pub async fn delete_favorite<T: AsRef<str>>(
        &self,
        photo_id: i64,
        user_id: T,
    ) -> Result<(), ErrorResponse> {
        let user_id = user_id.as_ref();
        query!(
            "delete from favorite_photos where photo_id = $1 and user_id = $2",
            photo_id,
            user_id
        )
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(internal_error)
    }

}