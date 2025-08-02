use crate::model::event_log::{EventLog, EventLogs};
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
        user_id: &str,
    ) -> Result<EventLogs, UserEventLogError> {
        let mut tx = self.pool.begin().await?;

        let ids = query!(
            "select min(event_id) as 'min_id!: i64', max(event_id) as 'max_id!: i64' from photos_event_log",
        ).map(|record| (record.min_id, record.max_id))
        .fetch_optional(tx.as_mut()).await?;

        let Some((min_event_id, max_event_id)) = ids else {
            return Err(UserEventLogError::NoEvents);
        };

        if last_event_id < min_event_id || last_event_id > max_event_id {
            return Err(UserEventLogError::InvalidEventId);
        }

        let event_logs = query_as!(
            EventLog,
            "select photo_id, data from photos_event_log where event_id > $1 and (user_id = $2 or user_id is null) order by event_id",
            last_event_id,
            user_id,
        )
            .fetch_all(tx.as_mut())
            .await?;

        Ok(EventLogs {
            event_log_id: max_event_id,
            events: event_logs,
        })
    }

    pub async fn delete_old_events(&self, last_rows_to_keep: u32) -> Result<(), sqlx::Error> {
        query!("delete from photos_event_log where event_id <= (select max(event_id) from photos_event_log) - $1", last_rows_to_keep)
            .execute(&self.pool)
            .await
            .map(|_| {})
    }
}

pub enum UserEventLogError {
    Database(sqlx::Error),
    InvalidEventId,
    NoEvents,
}

impl From<sqlx::Error> for UserEventLogError {
    fn from(err: sqlx::Error) -> Self {
        Self::Database(err)
    }
}
