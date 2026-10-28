use sqlx::{QueryBuilder, Sqlite, SqliteExecutor, query, query_scalar};
use std::collections::HashSet;

pub trait FavoritesRepo<'c>: SqliteExecutor<'c> {
    async fn get_favorite_photos(self, user_id: &str) -> sqlx::Result<HashSet<i64>> {
        query_scalar!(
            "select photo_id from favorite_photos where user_id = $1",
            user_id
        )
        .fetch_all(self)
        .await
        .map(|vec| vec.into_iter().collect())
    }

    async fn check_favorite(self, photo_id: i64, user_id: &str) -> sqlx::Result<bool> {
        query_scalar!(
            "select exists(select 1 from favorite_photos where photo_id = $1 and user_id = $2)",
            photo_id,
            user_id
        )
        .fetch_one(self)
        .await
        .map(|exists| exists != 0)
    }

    async fn check_favorites_for_ids(
        self,
        user_id: &str,
        photo_ids: &[i64],
    ) -> sqlx::Result<HashSet<i64>>
    where
        Self: Sized,
    {
        if photo_ids.is_empty() {
            return Ok(HashSet::new());
        }

        let mut qb: QueryBuilder<Sqlite> =
            QueryBuilder::new("select photo_id from favorite_photos where user_id = ");
        qb.push_bind(user_id).push(" and photo_id in (");
        let mut sep = qb.separated(", ");
        for id in photo_ids {
            sep.push_bind(*id);
        }
        sep.push_unseparated(")");

        let ids: Vec<i64> = qb.build_query_scalar().fetch_all(self).await?;
        Ok(ids.into_iter().collect())
    }

    async fn insert_favorite(self, photo_id: i64, user_id: &str) -> sqlx::Result<()> {
        query!(
            "insert into favorite_photos (photo_id, user_id) values ($1, $2) on conflict do nothing",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::tests::{insert_photo, insert_users, photo};
    use sqlx::SqlitePool;

    #[sqlx::test]
    async fn test_favorites(pool: SqlitePool) -> sqlx::Result<()> {
        insert_users(&pool).await;
        let mut ids = Vec::new();
        for name in ["a.jpg", "b.jpg", "c.jpg"] {
            ids.push(insert_photo(&pool, photo(Some("alice"), name)).await.id);
        }
        assert!(pool.get_favorite_photos("alice").await?.is_empty());
        assert!(!pool.check_favorite(ids[0], "alice").await?);

        pool.insert_favorite(ids[0], "alice").await?;
        pool.insert_favorite(ids[2], "alice").await?;
        // Idempotent
        pool.insert_favorite(ids[2], "alice").await?;
        pool.insert_favorite(ids[1], "bob").await?;

        // Per user
        assert_eq!(
            pool.get_favorite_photos("alice").await?,
            HashSet::from([ids[0], ids[2]])
        );
        assert_eq!(
            pool.get_favorite_photos("bob").await?,
            HashSet::from([ids[1]])
        );
        assert!(pool.check_favorite(ids[0], "alice").await?);
        assert!(!pool.check_favorite(ids[0], "bob").await?);
        assert!(pool.check_favorites_for_ids("alice", &[]).await?.is_empty());
        assert_eq!(
            pool.check_favorites_for_ids("alice", &[ids[0], ids[1]])
                .await?,
            HashSet::from([ids[0]])
        );

        pool.delete_favorite(ids[0], "alice").await?;
        // Deleting a missing favourite isn't an error
        pool.delete_favorite(ids[0], "alice").await?;
        assert_eq!(
            pool.get_favorite_photos("alice").await?,
            HashSet::from([ids[2]])
        );
        pool.insert_favorite(ids[0], "alice").await?;
        assert!(pool.check_favorite(ids[0], "alice").await?);

        Ok(())
    }
}
