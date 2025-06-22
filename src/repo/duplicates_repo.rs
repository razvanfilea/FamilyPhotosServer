use crate::model::photo::Photo;
use crate::model::user::PUBLIC_USER_ID;
use sqlx::{query, query_as, QueryBuilder, Sqlite, SqlitePool};
use std::num::ParseIntError;

#[derive(Clone)]
pub struct DuplicatesRepository {
    pool: SqlitePool,
}

impl DuplicatesRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_photos_without_hash(&self) -> Result<Vec<Photo>, sqlx::Error> {
        query_as!(
            Photo,
            "select p.* from photos p left join photos_extras e on p.id = e.id where e.sha is null"
        )
        .fetch_all(&self.pool)
        .await
    }
    
    pub async fn get_duplicates_for_user(
        &self,
        user_id: impl AsRef<str>,
    ) -> Result<Vec<Vec<i64>>, sqlx::Error> {
        let user_id = user_id.as_ref();
        query!(
            "select group_concat(e.id) as 'ids!: String' from photos_extras e
            join photos p on p.id = e.id
            where p.user_id = $1 or p.user_id = $2
            group by e.sha having count(*) > 1",
            user_id,
            PUBLIC_USER_ID
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

    pub async fn insert_hashes(&self, photos: &[(i64, String)]) -> Result<(), sqlx::Error> {
        let mut query_builder: QueryBuilder<Sqlite> =
            QueryBuilder::new("insert into photos_extras (id, sha) ");

        query_builder.push_values(photos, |mut b, (id, hash)| {
            b.push_bind(id).push_bind(hash);
        });

        query_builder.build().execute(&self.pool).await.map(|_| ())
    } 
}
