use axum_login::tower_sessions::session::{Id, Record};
use axum_login::tower_sessions::session_store;
use axum_login::tower_sessions::{ExpiredDeletion, SessionStore as TowerSessionStore};
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use std::str::FromStr;
use std::time::Duration;
use tower_sessions_sqlx_store::SqliteStore;
use tracing::info;

/// Initializes the Read and Write database pools with specific SQLite optimizations.
pub async fn init_pools(database_url: &str) -> (SqlitePool, SqlitePool) {
    let read_connections = std::thread::available_parallelism()
        .map(|p| (p.get() / 2).max(2) as u32)
        .unwrap_or(2);

    info!(
        "Initializing database pools: {} read connections, 1 write connection",
        read_connections
    );

    let connection_options = SqliteConnectOptions::from_str(database_url)
        .expect("Failed to parse Database URL")
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(60))
        .pragma("temp_store", "memory")
        .pragma("cache_size", "-20000");

    let read_pool = SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(read_connections)
        .connect_with(connection_options.clone().read_only(true))
        .await
        .expect("Failed to create Read-Only DB Pool");

    let write_pool = SqlitePoolOptions::new()
        .min_connections(0)
        .max_connections(1)
        .connect_with(connection_options.optimize_on_close(true, None))
        .await
        .expect("Failed to create Write DB Pool");

    (read_pool, write_pool)
}

/// Runs standard database migrations.
pub async fn run_migrations(pool: &SqlitePool) {
    sqlx::migrate!()
        .run(pool)
        .await
        .expect("Failed to run DB migrations");
}

/// Session store whose reads (every authenticated request) must not wait for the single write
/// connection, while all writes stay on that one writer.
#[derive(Clone, Debug)]
pub struct SessionStore {
    read: SqliteStore,
    write: SqliteStore,
}

impl SessionStore {
    pub fn new(read_pool: SqlitePool, write_pool: SqlitePool) -> Self {
        Self {
            read: SqliteStore::new(read_pool),
            write: SqliteStore::new(write_pool),
        }
    }

    pub async fn migrate(&self) -> sqlx::Result<()> {
        self.write.migrate().await
    }

    pub async fn delete_expired(&self) -> session_store::Result<()> {
        self.write.delete_expired().await
    }
}

#[async_trait::async_trait]
impl TowerSessionStore for SessionStore {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        self.write.create(record).await
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        self.write.save(record).await
    }

    async fn load(&self, session_id: &Id) -> session_store::Result<Option<Record>> {
        self.read.load(session_id).await
    }

    async fn delete(&self, session_id: &Id) -> session_store::Result<()> {
        self.write.delete(session_id).await
    }
}

#[cfg(test)]
mod tests {
    use sqlx::SqlitePool;

    const UP: &str = include_str!("../migrations/20261031120000_folder_created_index.up.sql");
    const DOWN: &str = include_str!("../migrations/20261031120000_folder_created_index.down.sql");

    async fn photo_indexes(pool: &SqlitePool) -> Vec<String> {
        sqlx::query_scalar(
            "select name from sqlite_master where type = 'index' and name like 'idx_photos_folder%' order by name",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    #[sqlx::test]
    async fn test_folder_created_index_migration(pool: SqlitePool) {
        assert_eq!(photo_indexes(&pool).await, ["idx_photos_folder_created"]);

        sqlx::raw_sql(DOWN).execute(&pool).await.unwrap();
        assert_eq!(photo_indexes(&pool).await, ["idx_photos_folder_id"]);

        for (name, created_at) in [
            ("old", "2020-01-02 03:04:05"),
            ("new", "2020-01-02T03:04:05Z"),
            ("fraction", "2020-01-02T03:04:05.25Z"),
            ("offset", "2020-01-02T01:04:05+02:00"),
        ] {
            sqlx::query("insert into photos (name, created_at, file_size) values (?, ?, 1)")
                .bind(name)
                .bind(created_at)
                .execute(&pool)
                .await
                .unwrap();
        }

        sqlx::raw_sql(UP).execute(&pool).await.unwrap();
        assert_eq!(photo_indexes(&pool).await, ["idx_photos_folder_created"]);
        let created_at: Vec<String> =
            sqlx::query_scalar("select created_at from photos order by id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            created_at,
            [
                "2020-01-02T03:04:05Z",
                "2020-01-02T03:04:05Z",
                "2020-01-02T03:04:05.25Z",
                "2020-01-01T23:04:05Z",
            ]
        );
    }
}
