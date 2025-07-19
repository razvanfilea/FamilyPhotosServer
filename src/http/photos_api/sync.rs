use crate::http::AppStateRef;
use crate::http::utils::{AuthSession, AxumResult};
use crate::model::event_log::EventLogNetwork;
use crate::utils::internal_error;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

pub fn router() -> Router<AppStateRef> {
    Router::new()
        .route("/full", get(full_photos_list))
        .route("/changes", get(partial_photos_list))
}

async fn full_photos_list(
    State(state): State<AppStateRef>,
    auth: AuthSession,
) -> AxumResult<impl IntoResponse> {
    let user = auth.user.ok_or(StatusCode::BAD_REQUEST)?;

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
) -> AxumResult<impl IntoResponse> {
    let user = auth.user.ok_or(StatusCode::BAD_REQUEST)?;
    let last_synced_event_id = query.last_synced_event_id;

    let events = state
        .event_log_repo
        .get_events_for_user(last_synced_event_id, user.id.as_str())
        .await
        .map_err(internal_error)?;

    let Some(events) = events else {
        return Err(StatusCode::CONFLICT.into());
    };

    let event_log_id = events
        .iter()
        .map(|event| event.event_id)
        .max()
        .unwrap_or(last_synced_event_id);
    let events: Vec<_> = events
        .into_iter()
        .map(|event| EventLogNetwork {
            photo_id: event.photo_id,
            data: event.data,
        })
        .collect();

    #[derive(Serialize)]
    struct PartialPhotosListResponse {
        event_log_id: i64,
        events: Vec<EventLogNetwork>,
    }

    Ok(Json(PartialPhotosListResponse {
        event_log_id,
        events,
    }))
}
