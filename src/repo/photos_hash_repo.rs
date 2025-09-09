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
