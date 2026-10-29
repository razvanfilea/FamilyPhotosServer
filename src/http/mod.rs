use crate::db::SessionStore;
use crate::previews::PreviewGenerator;
use crate::repo::users_repo::UsersRepository;
use crate::utils::storage_resolver::StorageResolver;
use axum::Router;
use axum::extract::{DefaultBodyLimit, Request};
use axum::http::{HeaderValue, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum_login::AuthManagerLayerBuilder;
use axum_login::tower_sessions::{Expiry, SessionManagerLayer};
use sqlx::SqlitePool;
use time::Duration;
use tokio::signal;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tower_http::{cors, trace};
use tracing::{Level, info, warn};

pub mod api;
mod auth;
pub mod context;
pub mod error;
mod extractors;
mod fragments;
mod page_context;
mod pages;
mod prefs;
mod session_activity;
mod template_into_response;
mod utils;
mod views;

#[cfg(test)]
mod tests;

macro_rules! content_security_policy {
    ($script_src:literal) => {
        concat!(
            "default-src 'self'; img-src 'self' data: blob:; media-src 'self' blob:; ",
            "style-src 'self' 'unsafe-inline'; script-src ",
            $script_src,
            "; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'"
        )
    };
}

/// The policy of every response unless a route sets its own
const CONTENT_SECURITY_POLICY: &str = content_security_policy!("'self'");

/// Only for the upload page and its hash worker script: BLAKE3 hashing compiles WebAssembly. A
/// dedicated worker takes its policy from its own script response, so both need it.
pub(super) const UPLOAD_CONTENT_SECURITY_POLICY: &str =
    content_security_policy!("'self' 'wasm-unsafe-eval'");

/// Path of the hash worker relative to the assets directory
const HASH_WORKER_PATH: &str = "/js/upload/hash-worker.js";

pub fn router(
    app_state: AppStateRef,
    session_store: SessionStore,
    allowed_origins: Vec<String>,
) -> Router {
    let session_layer = SessionManagerLayer::new(session_store)
        .with_expiry(Expiry::OnInactivity(Duration::days(60)));

    let auth_layer =
        AuthManagerLayerBuilder::new(app_state.users_repo.clone(), session_layer).build();

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
        .merge(pages::router(app_state))
        .nest("/api", api::router(app_state))
        .nest(
            "/assets",
            Router::new()
                .fallback_service(ServeDir::new("assets"))
                .layer(middleware::from_fn(asset_cache_headers)),
        )
        .fallback(pages::not_found)
        .layer(middleware::from_fn(session_activity::mark_session_active))
        // `if_not_present`: routes with their own policy (the upload page and its worker) keep it
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CONTENT_SECURITY_POLICY),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
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

/// Versioned assets (`?v=<ASSET_VERSION>`) are cached forever in release builds; everything else,
/// and all JS modules (whose relative imports can't carry the version), must be revalidated.
async fn asset_cache_headers(request: Request, next: Next) -> Response {
    let cache_control = asset_cache_control(
        request.uri().path(),
        request.uri().query(),
        AppState::ASSET_VERSION,
    );

    let is_hash_worker = request.uri().path() == HASH_WORKER_PATH;

    let mut response = next.run(request).await;
    if is_hash_worker {
        response.headers_mut().insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(UPLOAD_CONTENT_SECURITY_POLICY),
        );
    }
    // Never cache errors (e.g. a CSS file that isn't built yet) for a year
    let cache_control = if response.status().is_success() {
        cache_control
    } else {
        "no-cache"
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control),
    );
    response
}

/// `path` is relative to the assets directory, e.g. `/css/main.css`
fn asset_cache_control(path: &str, query: Option<&str>, asset_version: &str) -> &'static str {
    let immutable = asset_version != "dev"
        && !path.starts_with("/js/")
        && query.is_some_and(|query| {
            query
                .split('&')
                .any(|param| param.strip_prefix("v=") == Some(asset_version))
        });

    if immutable {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

pub struct AppState {
    pub storage: StorageResolver,
    pub read_pool: SqlitePool,
    pub write_pool: SqlitePool,
    pub users_repo: UsersRepository,
    pub previews: PreviewGenerator,
}

impl AppState {
    /// Hash of the static assets in release builds, `"dev"` otherwise
    pub const ASSET_VERSION: &str = env!("ASSET_VERSION");

    pub fn new(read_pool: SqlitePool, write_pool: SqlitePool, storage: StorageResolver) -> Self {
        Self {
            storage,
            users_repo: UsersRepository::new(read_pool.clone(), write_pool.clone()),
            read_pool,
            write_pool,
            previews: PreviewGenerator::new(),
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
