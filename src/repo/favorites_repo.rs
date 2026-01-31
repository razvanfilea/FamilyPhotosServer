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

    /// Check which of the given photo IDs are favorites for a user
    /// More efficient than fetching all favorites when you only need to check specific photos
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
}

impl<'c, E> FavoritesRepo<'c> for E where E: SqliteExecutor<'c> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::PhotosTransactionRepo;
    use crate::repo::tests::{create_test_photo, create_test_user, insert_test_user};
    use sqlx::SqlitePool;

    #[sqlx::test]
    async fn test_get_favorite_photos(pool: SqlitePool) -> sqlx::Result<()> {
        let user1 = create_test_user("user1", "User One");
        let user2 = create_test_user("user2", "User Two");
        insert_test_user(&pool, &user1).await?;
        insert_test_user(&pool, &user2).await?;

        // No favorites → empty HashSet
        let favs = pool.get_favorite_photos("user1").await?;
        assert!(favs.is_empty());

        // Insert some photos
        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), None, "p1.jpg"),
            create_test_photo(0, Some("user1"), None, "p2.jpg"),
            create_test_photo(0, Some("user1"), None, "p3.jpg"),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        let ids: Vec<i64> = sqlx::query_scalar!("select id from photos")
            .fetch_all(&pool)
            .await?;

        // Add favorites for user1
        pool.insert_favorite(ids[0], "user1").await?;
        pool.insert_favorite(ids[2], "user1").await?;

        // Add favorite for user2
        pool.insert_favorite(ids[1], "user2").await?;

        // User has favorites → HashSet contains those IDs
        let favs = pool.get_favorite_photos("user1").await?;
        assert_eq!(favs.len(), 2);
        assert!(favs.contains(&ids[0]));
        assert!(favs.contains(&ids[2]));

        // Other user's favorites not included
        assert!(!favs.contains(&ids[1]));

        // User2's favorites
        let favs = pool.get_favorite_photos("user2").await?;
        assert_eq!(favs.len(), 1);
        assert!(favs.contains(&ids[1]));

        Ok(())
    }

    #[sqlx::test]
    async fn test_check_favorite(pool: SqlitePool) -> sqlx::Result<()> {
        let user1 = create_test_user("user1", "User One");
        let user2 = create_test_user("user2", "User Two");
        insert_test_user(&pool, &user1).await?;
        insert_test_user(&pool, &user2).await?;

        let mut tx = pool.begin().await?;
        let photo = create_test_photo(0, Some("user1"), None, "test.jpg");
        let inserted = tx.insert_photo(&photo).await?;
        tx.commit().await?;

        // Photo not favorite → false
        let is_fav = pool.check_favorite(inserted.id, "user1").await?;
        assert!(!is_fav);

        // Add to favorites
        pool.insert_favorite(inserted.id, "user1").await?;

        // Photo is favorite → true
        let is_fav = pool.check_favorite(inserted.id, "user1").await?;
        assert!(is_fav);

        // Same photo for different user → false (independent)
        let is_fav = pool.check_favorite(inserted.id, "user2").await?;
        assert!(!is_fav);

        Ok(())
    }

    #[sqlx::test]
    async fn test_insert_delete_favorite(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let mut tx = pool.begin().await?;
        let photo = create_test_photo(0, Some("user1"), None, "test.jpg");
        let inserted = tx.insert_photo(&photo).await?;
        tx.commit().await?;

        // Insert new → success
        pool.insert_favorite(inserted.id, "user1").await?;
        let is_fav = pool.check_favorite(inserted.id, "user1").await?;
        assert!(is_fav);

        // Insert duplicate → unique constraint error
        let result = pool.insert_favorite(inserted.id, "user1").await;
        assert!(result.is_err());

        // Delete existing → success
        pool.delete_favorite(inserted.id, "user1").await?;
        let is_fav = pool.check_favorite(inserted.id, "user1").await?;
        assert!(!is_fav);

        // Delete non-existent → Ok (0 rows, no error)
        let result = pool.delete_favorite(inserted.id, "user1").await;
        assert!(result.is_ok());

        // Delete then re-insert → works
        pool.insert_favorite(inserted.id, "user1").await?;
        let is_fav = pool.check_favorite(inserted.id, "user1").await?;
        assert!(is_fav);

        Ok(())
    }

    #[sqlx::test]
    async fn test_check_favorites_for_ids(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), None, "p1.jpg"),
            create_test_photo(0, Some("user1"), None, "p2.jpg"),
            create_test_photo(0, Some("user1"), None, "p3.jpg"),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        let ids: Vec<i64> = sqlx::query_scalar!("select id from photos order by id")
            .fetch_all(&pool)
            .await?;

        // Empty input → empty result
        let result = pool.check_favorites_for_ids("user1", &[]).await?;
        assert!(result.is_empty());

        // No favorites → empty result
        let result = pool.check_favorites_for_ids("user1", &ids).await?;
        assert!(result.is_empty());

        // Add some favorites
        pool.insert_favorite(ids[0], "user1").await?;
        pool.insert_favorite(ids[2], "user1").await?;

        // Check specific IDs → only favorited ones returned
        let result = pool.check_favorites_for_ids("user1", &ids).await?;
        assert_eq!(result.len(), 2);
        assert!(result.contains(&ids[0]));
        assert!(result.contains(&ids[2]));
        assert!(!result.contains(&ids[1]));

        // Check subset of IDs
        let result = pool
            .check_favorites_for_ids("user1", &[ids[0], ids[1]])
            .await?;
        assert_eq!(result.len(), 1);
        assert!(result.contains(&ids[0]));

        Ok(())
    }
}
