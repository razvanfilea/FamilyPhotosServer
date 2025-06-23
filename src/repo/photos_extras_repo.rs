use crate::model::photo::Photo;
use crate::model::photo_extras::PhotoExtras;
use crate::model::user::PUBLIC_USER_ID;
use sqlx::{QueryBuilder, Sqlite, SqlitePool, query, query_as};
use std::num::ParseIntError;
use sqlx::types::Json;
use crate::model::exif_field::ExifFields;

#[derive(Clone)]
pub struct PhotosExtrasRepository {
    pool: SqlitePool,
}

impl PhotosExtrasRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_photos_without_extras(&self) -> Result<Vec<Photo>, sqlx::Error> {
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

    pub async fn get_photo_exif_json(&self, photo_id: i64) -> Result<Option<ExifFields>, sqlx::Error> {
        query!(
            "select exif_json as 'exif_json: Json<ExifFields>' from photos_extras where id = $1",
            photo_id
        )
        .map(|record| record.exif_json.map(|v| v.0))
        .fetch_optional(&self.pool)
        .await
        .map(Option::flatten)
    }

    pub async fn insert_hashes(&self, photos: &[PhotoExtras]) -> Result<(), sqlx::Error> {
        let mut query_builder: QueryBuilder<Sqlite> =
            QueryBuilder::new("insert into photos_extras (id, sha, exif_json) ");

        query_builder.push_values(photos, |mut b, photo| {
            b.push_bind(photo.id)
                .push_bind(&photo.sha)
                .push_bind(&photo.exif_json);
        });

        query_builder.build().execute(&self.pool).await.map(|_| ())
    }
}
