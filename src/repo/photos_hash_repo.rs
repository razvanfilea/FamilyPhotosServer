use crate::model::photo::Photo;
use crate::model::photo_hash::PhotoHash;
use sqlx::{QueryBuilder, Sqlite, SqlitePool, query, query_as};
use std::num::ParseIntError;

pub struct PhotosHashRepository {
    pool: SqlitePool,
}

impl PhotosHashRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_photos_without_hash(&self) -> Result<Vec<Photo>, sqlx::Error> {
        query_as!(
            Photo,
            "select p.* from photos p left join photos_hash e on p.id = e.photo_id
             where e.hash is null and p.trashed_on is null"
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_photo_with_hash(
        &self,
        hash: &[u8],
        user_id: Option<&str>,
    ) -> Result<Option<Photo>, sqlx::Error> {
        query_as!(
            Photo,
            "select p.* from photos p left join photos_hash e on p.id = e.photo_id
             where e.hash = $1 and (user_id = $2 or ($2 is null and user_id is null))",
            hash,
            user_id
        )
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn get_duplicates_for_user(
        &self,
        user_id: &str,
    ) -> Result<Vec<Vec<i64>>, sqlx::Error> {
        query!(
            "select group_concat(h.photo_id) as 'ids!: String' from photos_hash h
            join photos p on p.id = h.photo_id
            where p.user_id = $1 or p.user_id is null and p.trashed_on is null
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
        .fetch_all(&self.pool)
        .await
    }

    pub async fn insert_hashes(&self, photos: &[PhotoHash]) -> Result<(), sqlx::Error> {
        if photos.is_empty() {
            return Ok(());
        }

        QueryBuilder::<Sqlite>::new("insert or replace into photos_hash (photo_id, hash) ")
            .push_values(photos, |mut b, photo| {
                b.push_bind(photo.id).push_bind(&photo.hash);
            })
            .build()
            .execute(&self.pool)
            .await
            .map(|_| {})
    }
}
