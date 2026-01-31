use crate::model::photo::Photo;
use crate::model::photo_hash::PhotoHash;
use sqlx::{QueryBuilder, Sqlite, SqliteExecutor, query, query_as};
use std::num::ParseIntError;

pub trait PhotosHashRepo<'c>: SqliteExecutor<'c> {
    async fn get_photos_without_hash(self) -> sqlx::Result<Vec<Photo>> {
        query_as!(
            Photo,
            "select p.* from photos p left join photos_hash e on p.id = e.photo_id
             where e.hash is null and p.trashed_on is null"
        )
        .fetch_all(self)
        .await
    }

    async fn get_photo_with_hash(
        self,
        hash: &[u8],
        user_id: Option<&str>,
    ) -> sqlx::Result<Option<Photo>> {
        query_as!(
            Photo,
            "select p.* from photos p left join photos_hash e on p.id = e.photo_id
             where e.hash = $1 and (user_id = $2 or ($2 is null and user_id is null))",
            hash,
            user_id
        )
        .fetch_optional(self)
        .await
    }

    async fn get_duplicates_for_user(self, user_id: &str) -> sqlx::Result<Vec<Vec<i64>>> {
        // Note: Parentheses are important here for correct precedence
        // We want: (user's photos OR public photos) AND not trashed
        query!(
            "select group_concat(h.photo_id) as 'ids!: String' from photos_hash h
            join photos p on p.id = h.photo_id
            where (p.user_id = $1 or p.user_id is null) and p.trashed_on is null
            group by h.hash having count(*) > 1",
            user_id,
        )
        .map(|record| {
            record
                .ids
                .split(',')
                .map(|id| id.parse::<i64>())
                .collect::<Result<Vec<_>, ParseIntError>>()
                .expect("Photo id must be a valid i64")
        })
        .fetch_all(self)
        .await
    }

    async fn insert_hashes(self, photos: &[PhotoHash]) -> sqlx::Result<()> {
        if photos.is_empty() {
            return Ok(());
        }

        QueryBuilder::<Sqlite>::new("insert or replace into photos_hash (photo_id, hash) ")
            .push_values(photos, |mut b, photo| {
                b.push_bind(photo.id).push_bind(&photo.hash);
            })
            .build()
            .execute(self)
            .await
            .map(|_| {})
    }
}

impl<'c, E> PhotosHashRepo<'c> for E where E: SqliteExecutor<'c> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::PhotosTransactionRepo;
    use crate::repo::tests::{create_test_photo, create_test_user, insert_test_user};
    use sqlx::SqlitePool;
    use time::OffsetDateTime;

    #[sqlx::test]
    async fn test_get_photos_without_hash(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        // All have hash → empty
        // (but first we need photos)

        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), None, "p1.jpg"),
            create_test_photo(0, Some("user1"), None, "p2.jpg"),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        let ids: Vec<i64> = sqlx::query_scalar!("select id from photos")
            .fetch_all(&pool)
            .await?;

        // Without any hashes, both should be returned
        let without_hash = pool.get_photos_without_hash().await?;
        assert_eq!(without_hash.len(), 2);

        // Insert hash for one photo
        let hashes = vec![PhotoHash {
            id: ids[0],
            hash: vec![1, 2, 3, 4],
        }];
        pool.insert_hashes(&hashes).await?;

        // Some without → those returned
        let without_hash = pool.get_photos_without_hash().await?;
        assert_eq!(without_hash.len(), 1);
        assert_eq!(without_hash[0].id, ids[1]);

        // Insert hash for the other
        let hashes = vec![PhotoHash {
            id: ids[1],
            hash: vec![5, 6, 7, 8],
        }];
        pool.insert_hashes(&hashes).await?;

        // All have hash → empty
        let without_hash = pool.get_photos_without_hash().await?;
        assert!(without_hash.is_empty());

        // Trashed photo without hash → excluded (trashed_on IS NULL)
        let mut tx = pool.begin().await?;
        let mut trashed = create_test_photo(0, Some("user1"), None, "trashed.jpg");
        trashed.trashed_on = Some(OffsetDateTime::now_utc());
        tx.insert_photo(&trashed).await?;
        tx.commit().await?;

        // The trashed photo has no hash but should be excluded
        let without_hash = pool.get_photos_without_hash().await?;
        assert!(without_hash.is_empty());

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_photo_with_hash(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), None, "user1_photo.jpg"),
            create_test_photo(0, None, None, "public_photo.jpg"),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        // Get photos with specific user_id to ensure correct ID mapping
        let user1_photo =
            sqlx::query_as!(Photo, "select * from photos where name = 'user1_photo.jpg'")
                .fetch_one(&pool)
                .await?;
        let public_photo = sqlx::query_as!(
            Photo,
            "select * from photos where name = 'public_photo.jpg'"
        )
        .fetch_one(&pool)
        .await?;

        // Insert hashes
        let hashes = vec![
            PhotoHash {
                id: user1_photo.id,
                hash: vec![1, 2, 3, 4],
            },
            PhotoHash {
                id: public_photo.id,
                hash: vec![5, 6, 7, 8],
            },
        ];
        pool.insert_hashes(&hashes).await?;

        // Hash doesn't exist → None
        let result = pool
            .get_photo_with_hash(&[99, 99, 99], Some("user1"))
            .await?;
        assert!(result.is_none());

        // Hash exists for user → Some(photo)
        let result = pool
            .get_photo_with_hash(&[1, 2, 3, 4], Some("user1"))
            .await?;
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "user1_photo.jpg");

        // Hash exists for different user → None
        let result = pool
            .get_photo_with_hash(&[1, 2, 3, 4], Some("other_user"))
            .await?;
        assert!(result.is_none());

        // user_id=None → matches only public photos
        let result = pool.get_photo_with_hash(&[5, 6, 7, 8], None).await?;
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "public_photo.jpg");

        // user_id=None but hash is for private photo → None
        let result = pool.get_photo_with_hash(&[1, 2, 3, 4], None).await?;
        assert!(result.is_none());

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_duplicates_for_user(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), None, "p1.jpg"),
            create_test_photo(0, Some("user1"), None, "p2.jpg"),
            create_test_photo(0, None, None, "public.jpg"),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        let ids: Vec<i64> = sqlx::query_scalar!("select id from photos")
            .fetch_all(&pool)
            .await?;

        // No duplicates → empty vec
        let hashes = vec![
            PhotoHash {
                id: ids[0],
                hash: vec![1, 2, 3, 4],
            },
            PhotoHash {
                id: ids[1],
                hash: vec![5, 6, 7, 8],
            },
            PhotoHash {
                id: ids[2],
                hash: vec![9, 10, 11, 12],
            },
        ];
        pool.insert_hashes(&hashes).await?;

        let duplicates = pool.get_duplicates_for_user("user1").await?;
        assert!(duplicates.is_empty());

        // Create duplicates: p1 and public have same hash
        pool.insert_hashes(&[PhotoHash {
            id: ids[2],
            hash: vec![1, 2, 3, 4],
        }])
        .await?;

        // 2 photos with same hash → Vec containing [id1, id2]
        let duplicates = pool.get_duplicates_for_user("user1").await?;
        assert_eq!(duplicates.len(), 1);
        assert_eq!(duplicates[0].len(), 2);
        assert!(duplicates[0].contains(&ids[0]));
        assert!(duplicates[0].contains(&ids[2]));

        // Add another photo with same hash
        let mut tx = pool.begin().await?;
        let another = create_test_photo(0, Some("user1"), None, "p3.jpg");
        let inserted = tx.insert_photo(&another).await?;
        tx.commit().await?;

        pool.insert_hashes(&[PhotoHash {
            id: inserted.id,
            hash: vec![1, 2, 3, 4],
        }])
        .await?;

        // 3+ with same hash → all IDs grouped
        let duplicates = pool.get_duplicates_for_user("user1").await?;
        assert_eq!(duplicates.len(), 1);
        assert_eq!(duplicates[0].len(), 3);

        Ok(())
    }

    #[sqlx::test]
    async fn test_insert_hashes(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        // Empty vec → Ok (no-op)
        pool.insert_hashes(&[]).await?;

        let mut tx = pool.begin().await?;
        let photo = create_test_photo(0, Some("user1"), None, "test.jpg");
        let inserted = tx.insert_photo(&photo).await?;
        tx.commit().await?;

        // New hashes → inserted
        let hashes = vec![PhotoHash {
            id: inserted.id,
            hash: vec![1, 2, 3, 4],
        }];
        pool.insert_hashes(&hashes).await?;

        let result = pool
            .get_photo_with_hash(&[1, 2, 3, 4], Some("user1"))
            .await?;
        assert!(result.is_some());

        // Existing hash → replaced (INSERT OR REPLACE)
        let hashes = vec![PhotoHash {
            id: inserted.id,
            hash: vec![5, 6, 7, 8],
        }];
        pool.insert_hashes(&hashes).await?;

        // Old hash no longer works
        let result = pool
            .get_photo_with_hash(&[1, 2, 3, 4], Some("user1"))
            .await?;
        assert!(result.is_none());

        // New hash works
        let result = pool
            .get_photo_with_hash(&[5, 6, 7, 8], Some("user1"))
            .await?;
        assert!(result.is_some());

        Ok(())
    }
}
