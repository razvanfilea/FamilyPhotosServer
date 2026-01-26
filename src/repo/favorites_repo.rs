use sqlx::{SqliteExecutor, query, query_scalar};

pub trait FavoritesRepo<'c>: SqliteExecutor<'c> {
    async fn get_favorite_photos(self, user_id: &str) -> sqlx::Result<Vec<i64>> {
        query_scalar!(
            "select photo_id from favorite_photos where user_id = $1",
            user_id
        )
        .fetch_all(self)
        .await
    }

    async fn insert_favorite(self, photo_id: i64, user_id: &str) -> sqlx::Result<()> {
        query!(
            "insert into favorite_photos (photo_id, user_id) values ($1, $2)",
            photo_id,
            user_id
        )
        .execute(self)
        .await
        .map(|_| ())
    }

    async fn delete_favorite(self, photo_id: i64, user_id: &str) -> sqlx::Result<()> {
        query!(
            "delete from favorite_photos where photo_id = $1 and user_id = $2",
            photo_id,
            user_id
        )
        .execute(self)
        .await
        .map(|_| ())
    }
}

impl<'c, E> FavoritesRepo<'c> for E where E: SqliteExecutor<'c> {}
