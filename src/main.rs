use anyhow::Context;
use axum_login::tower_sessions::ExpiredDeletion;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use std::net::SocketAddr;
use std::str::FromStr;
use tokio::net::TcpListener;
use tower_sessions_sqlx_store::SqliteStore;
use tracing::info;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::http::AppState;
use crate::model::user::{PUBLIC_USER_ID, User};
use crate::repo::users_repo::UsersRepository;
use crate::utils::env_reader::EnvVariables;
use crate::utils::password_hash::{generate_hash_from_password, generate_random_password};
use crate::utils::storage_resolver::StorageResolver;

mod cli;
mod file_scan;
mod http;
mod model;
mod previews;
mod repo;
mod utils;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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

    sqlx::migrate!().run(&pool).await?;

    let app_state = AppState::new(pool.clone(), storage_resolver);

    // Migrate the sessions store and delete expired sessions
    let session_store = SqliteStore::new(pool);
    session_store
        .migrate()
        .await
        .expect("Failed to run schema migration for authentication");

    session_store.delete_expired().await?;

    // Create default public user
    create_public_user(&app_state.users_repo).await?;

    // Run the CLI
    if cli::run_cli(&app_state).await {
        return Ok(());
    }

    // Scan the storage directory for new photos in the background
    if vars.scan_new_files {
        file_scan::scan_new_files(app_state.clone());
    }

    info!("Server listening on port {}", vars.server_port);

    let http_service = http::router(app_state, session_store).into_make_service();
    let addr = SocketAddr::from(([127, 0, 0, 1], vars.server_port));
    let listener = TcpListener::bind(addr).await?;

    axum::serve(listener, http_service)
        .with_graceful_shutdown(http::shutdown_signal())
        .await
        .context("Failed to start server")
}

async fn create_public_user(repo: &UsersRepository) -> anyhow::Result<()> {
    if repo.get_user(PUBLIC_USER_ID).await.is_some() {
        return Ok(());
    }

    let user = User {
        id: PUBLIC_USER_ID.to_string(),
        name: PUBLIC_USER_ID.to_string(),
        password_hash: generate_hash_from_password(generate_random_password()),
    };

    info!("No users found, creating public user");

    repo.insert_user(&user)
        .await
        .context("Failed to create public user")
}
