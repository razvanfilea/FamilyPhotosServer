use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::get;
use axum_login::AuthManagerLayerBuilder;
use axum_login::tower_sessions::{Expiry, SessionManagerLayer};
use sqlx::SqlitePool;
use time::Duration;
use tokio::signal;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tower_http::{cors, trace};
use tower_sessions_sqlx_store::SqliteStore;
use tracing::{Level, warn};
use crate::repo::event_log::EventLogRepository;
use crate::repo::favorites_repo::FavoritesRepository;
use crate::repo::photos_hash_repo::PhotosHashRepository;
use crate::repo::photos_repo::PhotosRepository;
use crate::repo::users_repo::UsersRepository;
use crate::utils::storage_resolver::StorageResolver;

mod photos_api;
mod users_api;
mod utils;

pub fn router(app_state: AppStateRef, session_store: SqliteStore) -> Router {
    let session_layer = SessionManagerLayer::new(session_store)
        .with_expiry(Expiry::OnInactivity(Duration::days(90)));

    let auth_layer =
        AuthManagerLayerBuilder::new(app_state.users_repo.clone(), session_layer).build();

    Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .merge(users_api::router())
        .nest("/photos", photos_api::router(app_state))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
                .on_response(trace::DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(CorsLayer::new().allow_origin(cors::Any))
        .layer(auth_layer)
        .layer(DefaultBodyLimit::max(1024 * 1024 * 1024)) // 1GB
}

pub struct AppState {
    pub storage: StorageResolver,
    pub users_repo: UsersRepository,
    pub photos_repo: PhotosRepository,
    pub favorites_repo: FavoritesRepository,
    pub photo_hash_repo: PhotosHashRepository,
    pub event_log_repo: EventLogRepository,
}

impl AppState {
    pub fn new(pool: SqlitePool, storage: StorageResolver) -> Self {
        Self {
            storage,
            users_repo: UsersRepository::new(pool.clone()),
            photos_repo: PhotosRepository::new(pool.clone()),
            favorites_repo: FavoritesRepository::new(pool.clone()),
            photo_hash_repo: PhotosHashRepository::new(pool.clone()),
            event_log_repo: EventLogRepository::new(pool),
        }
    }
}

pub type AppStateRef = &'static AppState;

pub async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = signal::ctrl_c().await {
            warn!("Failed to install Ctrl+C handler: {e}")
        }
    };

    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
