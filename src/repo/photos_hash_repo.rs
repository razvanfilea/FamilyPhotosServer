use crate::model::photo::Photo;
use crate::model::photo_hash::PhotoHash;
use sqlx::{QueryBuilder, Sqlite, SqliteExecutor, query, query_as};
use std::num::ParseIntError;

pub trait PhotosHashRepo<'c>: SqliteExecutor<'c> {
    /// Returns up to `limit` photos without a hash and with an id greater than `last_id`, ordered by id.
    async fn get_photos_without_hash(self, last_id: i64, limit: i64) -> sqlx::Result<Vec<Photo>> {
        query_as!(
            Photo,
            "select p.* from photos p left join photos_hash e on p.id = e.photo_id
             where e.hash is null and p.trashed_on is null and p.id > $1
             order by p.id limit $2",
            last_id,
            limit
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

    async fn get_photo_with_hash_in_folder(
        self,
        hash: &[u8],
        folder_id: i64,
    ) -> sqlx::Result<Option<Photo>> {
        query_as!(
            Photo,
            "select p.* from photos p join photos_hash e on p.id = e.photo_id
             where e.hash = $1 and p.folder_id = $2",
            hash,
            folder_id
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

    async fn get_existing_photo_ids(self, ids: &[i64]) -> sqlx::Result<Vec<i64>> {
        let mut qb = QueryBuilder::<Sqlite>::new("select id from photos where id in (");
        let mut separated = qb.separated(", ");
        for id in ids {
            separated.push_bind(*id);
        }
        separated.push_unseparated(")");
        qb.build_query_scalar().fetch_all(self).await
    }

    /// The groups of photos with the same content among the user's own and the public photos
    /// that aren't trashed (at least two copies), each copy with its folder's name. Copies are
    /// ordered by id and groups by their first copy.
    async fn get_duplicate_photos_for_user(
        self,
        user_id: &str,
    ) -> sqlx::Result<Vec<Vec<(Photo, Option<String>)>>> {
        #[derive(sqlx::FromRow)]
        struct Row {
            #[sqlx(flatten)]
            photo: Photo,
            hash: Vec<u8>,
            folder_name: Option<String>,
        }

        let rows = sqlx::query_as::<_, Row>(
            "select p.*, h.hash as hash, f.name as folder_name from photos_hash h
            join photos p on p.id = h.photo_id
            left join folders f on f.id = p.folder_id
            where (p.user_id = $1 or p.user_id is null) and p.trashed_on is null
            and h.hash in (
                select h2.hash from photos_hash h2 join photos p2 on p2.id = h2.photo_id
                where (p2.user_id = $1 or p2.user_id is null) and p2.trashed_on is null
                group by h2.hash having count(*) > 1)
            order by h.hash, p.id",
        )
        .bind(user_id)
        .fetch_all(self)
        .await?;

        let mut groups: Vec<Vec<(Photo, Option<String>)>> = Vec::new();
        let mut current_hash: Option<Vec<u8>> = None;
        for row in rows {
            if current_hash.as_ref() != Some(&row.hash) {
                groups.push(Vec::new());
                current_hash = Some(row.hash);
            }
            groups
                .last_mut()
                .expect("a group was pushed")
                .push((row.photo, row.folder_name));
        }
        groups.sort_by_key(|group| group[0].0.id);

        Ok(groups)
    }

    async fn insert_hash(self, photo_hash: PhotoHash) -> sqlx::Result<()> {
        query!(
            "insert or replace into photos_hash (photo_id, hash) values ($1, $2)",
            photo_hash.id,
            photo_hash.hash
        )
        .execute(self)
        .await
        .map(|_| {})
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
    use crate::repo::tests::{insert_photo, insert_users, photo};
    use sqlx::SqlitePool;
    use time::OffsetDateTime;

    fn hash(id: i64, hash: u8) -> PhotoHash {
        PhotoHash {
            id,
            hash: vec![hash; 4],
        }
    }

    #[sqlx::test]
    async fn test_photo_hashes(pool: SqlitePool) -> sqlx::Result<()> {
        insert_users(&pool).await;
        let own = insert_photo(&pool, photo(Some("alice"), "own.jpg")).await;
        let public = insert_photo(&pool, photo(None, "public.jpg")).await;
        let bobs = insert_photo(&pool, photo(Some("bob"), "bob.jpg")).await;
        let trashed = Photo {
            trashed_on: Some(OffsetDateTime::now_utc()),
            ..photo(Some("alice"), "trashed.jpg")
        };
        let trashed = insert_photo(&pool, trashed).await;
        let without_hash = async || {
            let photos = pool.get_photos_without_hash(0, 100).await.unwrap();
            let mut ids: Vec<i64> = photos.into_iter().map(|p| p.id).collect();
            ids.sort();
            ids
        };

        // Trashed photos are never hashed
        assert_eq!(without_hash().await, [own.id, public.id, bobs.id]);

        // Keyset pagination: `limit` and `last_id` are respected, ordered by id
        let page = async |last_id, limit| {
            let photos = pool.get_photos_without_hash(last_id, limit).await.unwrap();
            photos.into_iter().map(|p| p.id).collect::<Vec<_>>()
        };
        assert_eq!(page(0, 1).await, [own.id]);
        assert_eq!(page(own.id, 1).await, [public.id]);
        assert_eq!(page(own.id, 100).await, [public.id, bobs.id]);
        assert!(page(bobs.id, 1).await.is_empty());
        pool.insert_hashes(&[]).await?;
        pool.insert_hashes(&[hash(own.id, 1), hash(public.id, 2)])
            .await?;
        pool.insert_hash(hash(bobs.id, 1)).await?;
        assert!(without_hash().await.is_empty());

        // Matches the owner exactly: `None` means public
        let find = async |value: u8, user_id| {
            let photo = pool
                .get_photo_with_hash(&[value; 4], user_id)
                .await
                .unwrap();
            photo.map(|p| p.id)
        };
        assert_eq!(find(1, Some("alice")).await, Some(own.id));
        assert_eq!(find(1, Some("bob")).await, Some(bobs.id));
        assert_eq!(find(1, Some("carol")).await, None);
        assert_eq!(find(2, None).await, Some(public.id));
        assert_eq!(find(1, None).await, None);
        assert_eq!(find(9, Some("alice")).await, None);

        // Replacing a hash
        pool.insert_hashes(&[hash(own.id, 3)]).await?;
        assert_eq!(find(1, Some("alice")).await, None);
        assert_eq!(find(3, Some("alice")).await, Some(own.id));

        // Duplicates among own + public, non-trashed photos
        assert!(pool.get_duplicates_for_user("alice").await?.is_empty());
        pool.insert_hashes(&[hash(public.id, 3), hash(trashed.id, 3)])
            .await?;
        let other = insert_photo(&pool, photo(Some("alice"), "other.jpg")).await;
        pool.insert_hash(hash(other.id, 3)).await?;
        let mut duplicates = pool.get_duplicates_for_user("alice").await?;
        duplicates.iter_mut().for_each(|group| group.sort());
        assert_eq!(duplicates, [vec![own.id, public.id, other.id]]);
        // Bob's photo only duplicates the public one for him
        pool.insert_hash(hash(bobs.id, 3)).await?;
        let mut duplicates = pool.get_duplicates_for_user("bob").await?;
        duplicates.iter_mut().for_each(|group| group.sort());
        assert_eq!(duplicates, [vec![public.id, bobs.id]]);

        Ok(())
    }
}
