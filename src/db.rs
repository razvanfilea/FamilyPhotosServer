use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use std::str::FromStr;
use std::time::Duration;
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
        .busy_timeout(Duration::from_secs(30))
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
