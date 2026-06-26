use crate::http::AppStateRef;
use crate::http::auth::AuthenticatedUser;
use crate::http::error::{HttpError, HttpResult};
use crate::model::event_log::EventLog;
use crate::model::photo::Photo;
use crate::repo::{
    FolderEventLogRepo, FolderEventLogTransactionRepo, FoldersRepo, PhotosRepo,
    PhotosTransactionRepo, UserEventLogError,
};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub fn router() -> Router<AppStateRef> {
    Router::new()
        .route("/full", get(full_photos_list))
        .route("/partial", get(partial_photos_list))
        .route("/folders", post(sync_folders))
}

/// GET /sync/full — returns all personal + public photos. Used as a fallback when the
/// client's cursor is stale (after receiving 409 from /sync/partial).
async fn full_photos_list(
    State(state): State<AppStateRef>,
    AuthenticatedUser(user): AuthenticatedUser,
) -> HttpResult<impl IntoResponse> {
    let mut tx = state.read_pool.begin().await?;

    let photos = tx.get_photos_by_user_and_public(user.id.as_str()).await?;

    Ok(Json(photos))
}

#[derive(Deserialize)]
struct PartialPhotosListQuery {
    last_synced_event_id: i64,
}

/// GET /sync/partial — returns delta events for personal + public photos since the given
/// cursor. Returns 409 if the cursor is stale (cleaned up) or invalid — client must
/// fall back to GET /sync/full.
async fn partial_photos_list(
    State(state): State<AppStateRef>,
    AuthenticatedUser(user): AuthenticatedUser,
    Query(query): Query<PartialPhotosListQuery>,
) -> HttpResult<impl IntoResponse> {
    let mut tx = state.read_pool.begin().await?;

    let events = tx
        .get_events_for_user(query.last_synced_event_id, user.id.as_str())
        .await;

    match events {
        Ok(events) => Ok(Json(events).into_response()),
        Err(UserEventLogError::NoEvents | UserEventLogError::InvalidEventId) => {
            Ok(StatusCode::CONFLICT.into_response())
        }
        Err(UserEventLogError::Database(err)) => Err(HttpError::Database(err)),
    }
}

#[derive(Serialize)]
struct SyncFolderResponse {
    id: i64,
    owner_id: Option<String>,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    can_upload: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    can_delete: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    latest_event_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    events: Option<Vec<EventLog>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    photos: Option<Vec<Photo>>,
}

/// POST /sync/folders — merged folder metadata + shared folder photo sync.
///
/// Request: `HashMap<folder_id, last_event_id>` — cursors for shared folders the client
/// tracks. Send `{}` to get just the folder list with no photo data.
///
/// Response: every folder the user can see (owned + public + shared via permissions).
/// Only shared folders (owner_id != current user) get photo data. For each shared folder
/// with a cursor in the request:
///   - cursor=0 or stale cursor → `photos` field with full photo list (client replaces local state)
///   - valid cursor > 0 → `events` field with deltas since that event_id
///
/// The client checks which field is present:
///   - `events` → apply deltas (data present = upsert, data null = delete)
///   - `photos` → replace all local photos for that folder_id
///
/// Personal/public folder photos are never included — those sync via GET /sync/partial.
///
/// Revocation: the client diffs the response folder list against locally stored shared
/// folders. Any locally tracked shared folder absent from the response was revoked —
/// delete its photos and cursor locally.
async fn sync_folders(
    State(state): State<AppStateRef>,
    AuthenticatedUser(user): AuthenticatedUser,
    Json(shared_cursors): Json<HashMap<i64, i64>>,
) -> HttpResult<impl IntoResponse> {
    let mut tx = state.read_pool.begin().await?;
    let folders = tx.as_mut().get_accessible_folders(user.id.as_str()).await?;

    let mut response: Vec<SyncFolderResponse> = Vec::with_capacity(folders.len());

    for folder in &folders {
        let is_shared = folder
            .owner_id
            .as_ref()
            .is_some_and(|owner_id| *owner_id != user.id);

        let (latest_event_id, events, photos) = match shared_cursors.get(&folder.id) {
            Some(&cursor) if is_shared && cursor > 0 => {
                let event_logs = tx.get_folder_events(folder.id, cursor).await?;
                if event_logs.events.is_empty() && event_logs.event_log_id < cursor {
                    // Cursor is stale (events were cleaned up) — fall back to full fetch
                    let photos = tx.as_mut().get_photos_in_folder(folder.id).await?;
                    let latest = tx.as_mut().get_latest_folder_event_id(folder.id).await?;
                    (Some(latest), None, Some(photos))
                } else {
                    (Some(event_logs.event_log_id), Some(event_logs.events), None)
                }
            }
            Some(_) if is_shared => {
                // cursor=0 — full fetch
                let photos = tx.as_mut().get_photos_in_folder(folder.id).await?;
                let latest = tx.as_mut().get_latest_folder_event_id(folder.id).await?;
                (Some(latest), None, Some(photos))
            }
            _ => (None, None, None),
        };

        response.push(SyncFolderResponse {
            id: folder.id,
            owner_id: folder.owner_id.clone(),
            name: folder.name.clone(),
            can_upload: if is_shared {
                Some(folder.can_upload)
            } else {
                None
            },
            can_delete: if is_shared {
                Some(folder.can_delete)
            } else {
                None
            },
            latest_event_id,
            events,
            photos,
        });
    }

    Ok(Json(response))
}
