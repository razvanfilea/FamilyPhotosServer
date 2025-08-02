use crate::http::AppStateRef;
use crate::http::utils::{AuthSession, AxumResult};
use crate::repo::event_log::UserEventLogError;
use crate::utils::internal_error;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use tracing::error;

pub fn router() -> Router<AppStateRef> {
    Router::new()
        .route("/full", get(full_photos_list))
        .route("/changes", get(partial_photos_list))
}

async fn full_photos_list(
    State(state): State<AppStateRef>,
    auth: AuthSession,
) -> AxumResult<impl IntoResponse> {
    let user = auth.user.ok_or(StatusCode::UNAUTHORIZED)?;

    let photos = state
        .photos_repo
        .get_photos_by_user_and_public(user.id.as_str())
        .await
        .map_err(internal_error)?;

    Ok(Json(photos))
}

#[derive(Deserialize)]
struct PartialPhotosListQuery {
    last_synced_event_id: i64,
}

async fn partial_photos_list(
    State(state): State<AppStateRef>,
    auth: AuthSession,
    Query(query): Query<PartialPhotosListQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    let user = auth.user.ok_or(StatusCode::UNAUTHORIZED)?;
    let last_synced_event_id = query.last_synced_event_id;

    let events = state
        .event_log_repo
        .get_events_for_user(last_synced_event_id, user.id.as_str())
        .await;

    match events {
        Ok(events) => Ok(Json(events)),
        Err(UserEventLogError::NoEvents | UserEventLogError::InvalidEventId) => {
            Err(StatusCode::CONFLICT)
        }
        Err(UserEventLogError::Database(err)) => {
            error!("Database error: {}", err);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
