use crate::model::event_log::EventLog;
use crate::model::user::PUBLIC_USER_ID;
use sqlx::{SqlitePool, query, query_as};

pub struct EventLogRepository {
    pool: SqlitePool,
}

impl EventLogRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_events_for_user(
        &self,
        last_event_id: i64,
        user_id: impl AsRef<str>,
    ) -> Result<Option<Vec<EventLog>>, sqlx::Error> {
        let user_id = user_id.as_ref();
        let mut tx = self.pool.begin().await?;

        let min_event_id = query!(
            "select min(event_id) as 'id: i64' from photos_event_log where user_id = $1 or user_id = $2 or user_id is null",
            user_id,
            PUBLIC_USER_ID
        )
        .fetch_one(tx.as_mut()).await?.id;

        if last_event_id < min_event_id.unwrap_or_default() {
            return Ok(None);
        }

        let event_logs = query_as!(
            EventLog,
            "select event_id, photo_id, data from photos_event_log where event_id > $1 and (user_id = $2 or user_id = $3 or user_id is null) order by event_id",
            last_event_id,
            user_id,
            PUBLIC_USER_ID
        )
            .fetch_all(tx.as_mut())
            .await?;

        tx.commit().await?;

        Ok(Some(event_logs))
    }

    pub async fn delete_older_than(&self, last_rows_to_keep: i64) -> Result<(), sqlx::Error> {
        query!("delete from photos_event_log where event_id <= (select max(event_id) from photos_event_log) - $1", last_rows_to_keep)
            .execute(&self.pool)
            .await
            .map(|_| {})
    }
}
