use crate::http::AppState;
use crate::tasks::start_period_tasks;
use crate::utils::env_reader::EnvVariables;
use crate::utils::storage_resolver::StorageResolver;
use axum_login::tower_sessions::ExpiredDeletion;
use mimalloc::MiMalloc;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use std::net::SocketAddr;
use std::str::FromStr;
use tokio::net::TcpListener;
use tower_sessions_sqlx_store::SqliteStore;
use tracing::info;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

mod cli;
mod http;
mod model;
mod previews;
mod repo;
mod tasks;
mod utils;

#[tokio::main]
async fn main() {
    let vars = EnvVariables::get_all();
    // Creates the necessary folders
    let storage_resolver = StorageResolver::new(vars.storage_path, vars.previews_path);

    // Logging
    tracing_subscriber::registry()
        .with(EnvFilter::new(std::env::var("RUST_LOG").unwrap_or_else(
            |_| "info,axum_login=off,tower_sessions=off,sqlx=warn,tower_http=info".into(),
        )))
        .with(tracing_subscriber::fmt::layer().compact())
        .init();

    let connection_options = SqliteConnectOptions::from_str(&vars.database_url)
        .expect("Failed to parse Database URL")
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .pragma("temp_store", "memory")
        .pragma("cache_size", "-20000")
        .optimize_on_close(true, None);

    let pool = SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(4)
        .connect_with(connection_options)
        .await
        .expect("Failed to create DB Pool");

    sqlx::migrate!()
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    // Migrate the sessions store and delete expired sessions
    let session_store = SqliteStore::new(pool.clone());
    session_store
        .migrate()
        .await
        .expect("Failed to run schema migration for authentication");

    let app_state = AppState::new(pool, storage_resolver);
    let app_state = Box::leak(Box::new(app_state));

    session_store
        .delete_expired()
        .await
        .expect("Failed to delete expired sessions");

    // Run the CLI
    if cli::run_cli(app_state).await {
        return;
    }

    start_period_tasks(app_state, vars.scan_new_files);

    info!("Server listening on port {}", vars.server_port);

    let http_service = http::router(app_state, session_store).into_make_service();
    let addr = SocketAddr::from(([0, 0, 0, 0], vars.server_port));
    let listener = TcpListener::bind(addr)
        .await
        .expect("Failed to bind to port");

    axum::serve(listener, http_service)
        .with_graceful_shutdown(http::shutdown_signal())
        .await
        .expect("Failed to start server")
}
