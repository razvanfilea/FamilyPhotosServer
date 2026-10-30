use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use axum_login::tower_sessions::Session;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;

const LAST_SEEN_KEY: &str = "last_seen";

/// tower-sessions only extends `Expiry::OnInactivity` sessions when they are saved, and a session
/// is only saved when modified. Touching it once a day (after the handler, so a login is saved
/// once with it) keeps active users logged in without writing on every request.
pub async fn mark_session_active(request: Request, next: Next) -> Response {
    let session = request.extensions().get::<Session>().cloned();
    let response = next.run(request).await;
    if let Some(session) = session
        && !session.is_empty().await
    {
        let today = days_since_epoch();
        match session.get::<i64>(LAST_SEEN_KEY).await {
            Ok(Some(last_seen)) if last_seen >= today => {}
            Ok(_) => {
                if let Err(e) = session.insert(LAST_SEEN_KEY, today).await {
                    warn!("Failed to mark session as active: {e}");
                }
            }
            Err(e) => warn!("Failed to read session activity: {e}"),
        }
    }
    response
}

fn days_since_epoch() -> i64 {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    (secs / 86_400) as i64
}
