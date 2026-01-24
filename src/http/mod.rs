use crate::repo::users_repo::UsersRepository;
use crate::utils::storage_resolver::StorageResolver;
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::HeaderValue;
use axum::routing::get;
use axum_login::tower_sessions::{Expiry, SessionManagerLayer};
use axum_login::{AuthManagerLayerBuilder, login_required};
use sqlx::SqlitePool;
use time::Duration;
use tokio::signal;
use tokio::sync::Mutex;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tower_http::{cors, trace};
use tower_sessions_sqlx_store::SqliteStore;
use tracing::{Level, info, warn};

mod error;
mod extractors;
mod photos_api;
mod shared_api;
mod users_api;
mod utils;

pub fn router(
    app_state: AppStateRef,
    session_store: SqliteStore,
    allowed_origins: Vec<String>,
) -> Router {
    let session_layer = SessionManagerLayer::new(session_store)
        .with_expiry(Expiry::OnInactivity(Duration::days(60)));

    let auth_layer =
        AuthManagerLayerBuilder::new(app_state.users_repo.clone(), session_layer).build();

    let authenticated_router = Router::new()
        .nest("/photos", photos_api::router(app_state))
        .route_layer(login_required!(UsersRepository));

    let cors_layer = if allowed_origins.is_empty() {
        info!("CORS: Allowing any origin");
        CorsLayer::new().allow_origin(cors::Any)
    } else {
        info!("CORS: Allowing origins: {:?}", allowed_origins);
        let origins: Vec<_> = allowed_origins
            .iter()
            .filter_map(|s| s.parse().ok())
            .collect();
        CorsLayer::new().allow_origin(AllowOrigin::list(origins))
    };

    Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .merge(users_api::router())
        .merge(authenticated_router)
        .nest("/shared", shared_api::router(app_state))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
                .on_response(trace::DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(cors_layer)
        .layer(auth_layer)
        .layer(DefaultBodyLimit::max(1024 * 1024 * 1024)) // 1GB
}

pub struct AppState {
    pub storage: StorageResolver,
    pub pool: SqlitePool,
    pub users_repo: UsersRepository,
    pub preview_generation: Mutex<()>,
}

impl AppState {
    pub fn new(pool: SqlitePool, storage: StorageResolver) -> Self {
        Self {
            storage,
            users_repo: UsersRepository::new(pool.clone()),
            pool,
            preview_generation: Mutex::new(()),
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
