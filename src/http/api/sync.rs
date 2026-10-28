use crate::http::AppStateRef;
use crate::http::auth::AuthenticatedUser;
use crate::http::error::{HttpError, HttpResult};
use crate::model::event_log::{EventLog, EventLogs};
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
use sqlx::SqliteTransaction;
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
        Err(UserEventLogError::InvalidEventId) => Ok(StatusCode::CONFLICT.into_response()),
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

enum SharedFolderSync {
    Events(EventLogs),
    Photos {
        latest_event_id: i64,
        photos: Vec<Photo>,
    },
}

async fn sync_shared_folder(
    tx: &mut SqliteTransaction<'_>,
    folder_id: i64,
    cursor: i64,
) -> sqlx::Result<SharedFolderSync> {
    if cursor > 0
        && let Some(event_logs) = tx.get_folder_events(folder_id, cursor).await?
    {
        return Ok(SharedFolderSync::Events(event_logs));
    }

    let photos = tx.as_mut().get_photos_in_folder(folder_id).await?;
    let latest_event_id = tx.as_mut().get_latest_folder_event_id(folder_id).await?;
    Ok(SharedFolderSync::Photos {
        latest_event_id,
        photos,
    })
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
///   - valid cursor > 0 → `events` field with deltas since that event_id (possibly empty)
///
/// A cursor > 0 is stale when the folder's oldest remaining event is newer than the cursor
/// (cleanup deleted events the client has not seen), when it is ahead of the folder's
/// newest event, or when the folder has no events at all.
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
            Some(&cursor) if is_shared => {
                match sync_shared_folder(&mut tx, folder.id, cursor).await? {
                    SharedFolderSync::Events(event_logs) => {
                        (Some(event_logs.event_log_id), Some(event_logs.events), None)
                    }
                    SharedFolderSync::Photos {
                        latest_event_id,
                        photos,
                    } => (Some(latest_event_id), None, Some(photos)),
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::tests::{create_test_folder, insert_photo, insert_users, photo};
    use sqlx::SqlitePool;

    /// Adds `count` photos (one folder event each) to the folder
    async fn add_photos(pool: &SqlitePool, folder_id: i64, count: usize) {
        for i in 0..count {
            let photo = Photo {
                folder_id: Some(folder_id),
                ..photo(Some("alice"), &format!("p{i}.jpg"))
            };
            insert_photo(pool, photo).await;
        }
    }

    async fn folder_event_ids(pool: &SqlitePool, folder_id: i64) -> sqlx::Result<Vec<i64>> {
        sqlx::query_scalar!(
            "select event_id from folder_event_log where folder_id = $1 order by event_id",
            folder_id
        )
        .fetch_all(pool)
        .await
    }

    /// What the sync of a shared folder must return
    #[derive(Debug, PartialEq)]
    enum Expected {
        /// The deltas: the number of events
        Events(usize),
        /// The full list: the number of photos
        Photos(usize),
    }

    struct Case {
        name: &'static str,
        /// Photos (one folder event each) created before the sync
        photos: usize,
        /// Keep only the last n events of each folder before the sync
        trim: Option<u32>,
        /// The cursor, from the folder's event ids before the trimming
        cursor: fn(&[i64]) -> i64,
        /// Photos added after the cursor was chosen
        added_after: usize,
        expected: Expected,
    }

    #[sqlx::test]
    async fn test_sync_shared_folder(pool: SqlitePool) -> sqlx::Result<()> {
        insert_users(&pool).await;

        let cases = [
            Case {
                name: "cursor 0 returns the photos",
                photos: 3,
                trim: None,
                cursor: |_| 0,
                added_after: 0,
                expected: Expected::Photos(3),
            },
            Case {
                name: "up to date cursor returns no events",
                photos: 3,
                trim: None,
                cursor: |ids| *ids.last().unwrap(),
                added_after: 0,
                expected: Expected::Events(0),
            },
            Case {
                name: "valid cursor returns the new events",
                photos: 3,
                trim: None,
                cursor: |ids| *ids.last().unwrap(),
                added_after: 2,
                expected: Expected::Events(2),
            },
            Case {
                name: "cursor at the oldest surviving event keeps the events",
                photos: 5,
                trim: Some(3),
                cursor: |ids| ids[2],
                added_after: 0,
                expected: Expected::Events(2),
            },
            Case {
                name: "cursor before the oldest surviving event returns the photos",
                photos: 5,
                trim: Some(2),
                cursor: |ids| ids[1],
                added_after: 0,
                expected: Expected::Photos(5),
            },
            Case {
                name: "cursor of the newest deleted event returns the photos",
                photos: 5,
                trim: Some(2),
                cursor: |ids| ids[2],
                added_after: 0,
                expected: Expected::Photos(5),
            },
            Case {
                name: "cursor ahead of the newest event returns the photos",
                photos: 3,
                trim: None,
                cursor: |ids| ids.last().unwrap() + 100,
                added_after: 0,
                expected: Expected::Photos(3),
            },
            Case {
                name: "folder without events returns the photos",
                photos: 0,
                trim: None,
                cursor: |_| 7,
                added_after: 0,
                expected: Expected::Photos(0),
            },
        ];

        for (i, case) in cases.iter().enumerate() {
            let folder_id = create_test_folder(&pool, Some("alice"), &format!("shared{i}"))
                .await
                .id;
            add_photos(&pool, folder_id, case.photos).await;
            let ids = folder_event_ids(&pool, folder_id).await?;
            if let Some(trim) = case.trim {
                pool.delete_old_folder_events(trim).await?;
            }
            let cursor = (case.cursor)(&ids);
            add_photos(&pool, folder_id, case.added_after).await;
            let latest = folder_event_ids(&pool, folder_id)
                .await?
                .last()
                .copied()
                .unwrap_or(0);

            let mut tx = pool.begin().await?;
            let result = sync_shared_folder(&mut tx, folder_id, cursor).await?;
            let (actual, actual_latest) = match result {
                SharedFolderSync::Events(logs) => {
                    (Expected::Events(logs.events.len()), logs.event_log_id)
                }
                SharedFolderSync::Photos {
                    latest_event_id,
                    photos,
                } => (Expected::Photos(photos.len()), latest_event_id),
            };
            assert_eq!(actual, case.expected, "{}", case.name);
            assert_eq!(actual_latest, latest, "{}: latest event id", case.name);
        }

        Ok(())
    }
}
