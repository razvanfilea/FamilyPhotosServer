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

/// Syncs a single shared folder: deltas for a valid cursor, the full photo list for
/// cursor <= 0 or a stale cursor.
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
    use crate::repo::tests::{
        create_test_folder, create_test_photo, create_test_user, insert_test_user,
    };
    use sqlx::SqlitePool;

    /// Creates a folder with `count` photos (one folder event each); returns the folder id.
    async fn setup_folder(pool: &SqlitePool, count: usize) -> sqlx::Result<i64> {
        insert_test_user(pool, &create_test_user("owner", "Owner")).await?;
        let folder = create_test_folder(pool, Some("owner"), "shared").await;
        add_photos(pool, folder.id, count).await?;
        Ok(folder.id)
    }

    async fn add_photos(pool: &SqlitePool, folder_id: i64, count: usize) -> sqlx::Result<()> {
        let photos: Vec<_> = (0..count)
            .map(|i| create_test_photo(0, Some("owner"), Some(folder_id), &format!("p{i}.jpg")))
            .collect();
        let mut tx = pool.begin().await?;
        tx.insert_photos(&photos).await?;
        tx.commit().await
    }

    async fn sync(
        pool: &SqlitePool,
        folder_id: i64,
        cursor: i64,
    ) -> sqlx::Result<SharedFolderSync> {
        let mut tx = pool.begin().await?;
        sync_shared_folder(&mut tx, folder_id, cursor).await
    }

    async fn folder_event_ids(pool: &SqlitePool, folder_id: i64) -> sqlx::Result<Vec<i64>> {
        sqlx::query_scalar!(
            "select event_id from folder_event_log where folder_id = $1 order by event_id",
            folder_id
        )
        .fetch_all(pool)
        .await
    }

    fn expect_photos(result: SharedFolderSync, expected_latest: i64, expected_count: usize) {
        match result {
            SharedFolderSync::Photos {
                latest_event_id,
                photos,
            } => {
                assert_eq!(latest_event_id, expected_latest);
                assert_eq!(photos.len(), expected_count);
            }
            SharedFolderSync::Events(_) => panic!("expected the full photo list"),
        }
    }

    fn expect_events(result: SharedFolderSync, expected_latest: i64, expected_count: usize) {
        match result {
            SharedFolderSync::Events(logs) => {
                assert_eq!(logs.event_log_id, expected_latest);
                assert_eq!(logs.events.len(), expected_count);
            }
            SharedFolderSync::Photos { .. } => panic!("expected events"),
        }
    }

    #[sqlx::test]
    async fn test_sync_cursor_zero_returns_photos(pool: SqlitePool) -> sqlx::Result<()> {
        let folder_id = setup_folder(&pool, 3).await?;
        let latest = *folder_event_ids(&pool, folder_id).await?.last().unwrap();

        expect_photos(sync(&pool, folder_id, 0).await?, latest, 3);
        Ok(())
    }

    #[sqlx::test]
    async fn test_sync_up_to_date_cursor_returns_empty_events(
        pool: SqlitePool,
    ) -> sqlx::Result<()> {
        let folder_id = setup_folder(&pool, 3).await?;
        let latest = *folder_event_ids(&pool, folder_id).await?.last().unwrap();

        expect_events(sync(&pool, folder_id, latest).await?, latest, 0);
        Ok(())
    }

    #[sqlx::test]
    async fn test_sync_valid_cursor_returns_new_events(pool: SqlitePool) -> sqlx::Result<()> {
        let folder_id = setup_folder(&pool, 3).await?;
        let cursor = *folder_event_ids(&pool, folder_id).await?.last().unwrap();
        add_photos(&pool, folder_id, 2).await?;
        let latest = *folder_event_ids(&pool, folder_id).await?.last().unwrap();

        expect_events(sync(&pool, folder_id, cursor).await?, latest, 2);
        Ok(())
    }

    #[sqlx::test]
    async fn test_sync_valid_cursor_after_trimming_keeps_events(
        pool: SqlitePool,
    ) -> sqlx::Result<()> {
        let folder_id = setup_folder(&pool, 5).await?;
        let ids = folder_event_ids(&pool, folder_id).await?;
        pool.delete_old_folder_events(3).await?;

        // The cursor is the oldest remaining event: nothing after it was deleted
        expect_events(sync(&pool, folder_id, ids[2]).await?, ids[4], 2);
        Ok(())
    }

    #[sqlx::test]
    async fn test_sync_trimmed_cursor_returns_photos(pool: SqlitePool) -> sqlx::Result<()> {
        let folder_id = setup_folder(&pool, 5).await?;
        let ids = folder_event_ids(&pool, folder_id).await?;
        pool.delete_old_folder_events(2).await?;
        assert_eq!(folder_event_ids(&pool, folder_id).await?, &ids[3..]);

        // Events after the cursor were deleted, the survivors are not a complete delta
        expect_photos(sync(&pool, folder_id, ids[1]).await?, ids[4], 5);
        // Same for the cursor of the newest deleted event
        expect_photos(sync(&pool, folder_id, ids[2]).await?, ids[4], 5);
        Ok(())
    }

    #[sqlx::test]
    async fn test_sync_cursor_ahead_of_max_returns_photos(pool: SqlitePool) -> sqlx::Result<()> {
        let folder_id = setup_folder(&pool, 3).await?;
        let latest = *folder_event_ids(&pool, folder_id).await?.last().unwrap();

        expect_photos(sync(&pool, folder_id, latest + 100).await?, latest, 3);
        Ok(())
    }

    #[sqlx::test]
    async fn test_sync_folder_without_events_returns_photos(pool: SqlitePool) -> sqlx::Result<()> {
        let folder_id = setup_folder(&pool, 0).await?;

        expect_photos(sync(&pool, folder_id, 7).await?, 0, 0);
        Ok(())
    }
}
