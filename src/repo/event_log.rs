use crate::model::photo::Photo;
use sqlx::{QueryBuilder, Sqlite, SqliteExecutor, query};

pub trait EventLogRepo<'c>: SqliteExecutor<'c> {
    async fn insert_event_log(
        self,
        photo_id: i64,
        user_id: Option<&str>,
        photo: Option<&Photo>,
    ) -> sqlx::Result<()> {
        let serialized_data = match photo {
            Some(photo) => Some(photo_to_json_bytes(photo)?),
            None => None,
        };

        query!(
            "insert into photos_event_log (photo_id, user_id, data) values ($1, $2, $3)",
            photo_id,
            user_id,
            serialized_data
        )
        .execute(self)
        .await
        .map(|_| ())
    }

    async fn insert_creation_event_logs(self, photos: &[Photo]) -> sqlx::Result<()> {
        if photos.is_empty() {
            // An empty vector would cause a SQL syntax error
            return Ok(());
        }

        let photos = photos
            .iter()
            .map(|photo| photo_to_json_bytes(photo).map(|data| (photo, data)))
            .collect::<sqlx::Result<Vec<_>>>()?;

        QueryBuilder::<Sqlite>::new("insert into photos_event_log (photo_id, user_id, data) ")
            .push_values(photos, |mut b, (photo, serialized_data)| {
                b.push_bind(photo.id)
                    .push_bind(&photo.user_id)
                    .push_bind(serialized_data);
            })
            .build()
            .execute(self)
            .await
            .map(|_| ())
    }

    async fn insert_deletion_event_logs(self, photo_ids: &[i64]) -> sqlx::Result<()> {
        QueryBuilder::<Sqlite>::new("insert into photos_event_log (photo_id) ")
            .push_values(photo_ids, |mut b, photo_id| {
                b.push_bind(photo_id);
            })
            .build()
            .execute(self)
            .await
            .map(|_| ())
    }

    async fn delete_old_events(self, last_rows_to_keep: u32) -> Result<(), sqlx::Error> {
        query!("delete from photos_event_log where event_id <= (select max(event_id) from photos_event_log) - $1", last_rows_to_keep)
            .execute(self)
            .await
            .map(|_| {})
    }
}

impl<'c, E> EventLogRepo<'c> for E where E: SqliteExecutor<'c> {}

fn photo_to_json_bytes(photo: &Photo) -> sqlx::Result<Vec<u8>> {
    serde_json::to_vec(&photo).map_err(|e| sqlx::Error::Encode(e.into()))
}
