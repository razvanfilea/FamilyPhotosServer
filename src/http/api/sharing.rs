use std::collections::HashMap;

use axum::{
    Json, Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::{delete, get, post, put},
};

use time::OffsetDateTime;
use tracing::info;

use crate::http::error::{HttpError, HttpResult, is_unique_violation};
use crate::model::folder_permission::{CreateShareRequest, ShareResponse};
use crate::repo::{FolderPermissionsRepo, FoldersRepo};
use crate::{
    http::{AppStateRef, auth::AuthenticatedUser},
    model::folder_permission::UpdateShareRequest,
};

pub fn router() -> Router<AppStateRef> {
    Router::new()
        .route("/", get(list_shares))
        .route("/", post(create_share))
        .route("/{share_id}", put(update_share))
        .route("/{share_id}", delete(revoke_share))
        .route("/folder/{folder_id}", get(list_folder_shares))
}

async fn list_shares(
    State(state): State<AppStateRef>,
    AuthenticatedUser(user): AuthenticatedUser,
) -> HttpResult<impl IntoResponse> {
    let mut tx = state.read_pool.begin().await?;

    let shares = tx.get_shares_by_owner(&user.id).await?;
    let folder_map = tx.get_folder_name_map().await?;
    let responses: Vec<ShareResponse> = shares
        .into_iter()
        .map(|s| ShareResponse::from_permission(s, &folder_map))
        .collect();

    Ok(Json(responses))
}

async fn list_folder_shares(
    State(state): State<AppStateRef>,
    AuthenticatedUser(user): AuthenticatedUser,
    Path(folder_id): Path<i64>,
) -> HttpResult<impl IntoResponse> {
    let mut tx = state.read_pool.begin().await?;
    let Some(folder) = tx.get_accessible_folder(&user.id, folder_id).await? else {
        return Err(HttpError::NotFound);
    };

    let shares = tx.get_shares_for_folder(folder_id).await?;
    let folder_map = HashMap::from([(folder_id, folder.name)]);
    let responses: Vec<ShareResponse> = shares
        .into_iter()
        .map(|s| ShareResponse::from_permission(s, &folder_map))
        .collect();

    Ok(Json(responses))
}

fn ensure_in_the_future(expires_at: OffsetDateTime) -> HttpResult<()> {
    if expires_at <= OffsetDateTime::now_utc() {
        return Err(HttpError::BadRequest(
            "Expiry must be in the future".to_string(),
        ));
    }
    Ok(())
}

async fn create_share(
    State(state): State<AppStateRef>,
    AuthenticatedUser(user): AuthenticatedUser,
    Json(request): Json<CreateShareRequest>,
) -> HttpResult<impl IntoResponse> {
    if let Some(expires_at) = request.expires_at {
        ensure_in_the_future(expires_at)?;
    }

    let mut tx = state.write_pool.begin().await?;

    let Some(folder) = tx
        .get_accessible_folder(&user.id, request.folder_id)
        .await?
    else {
        return Err(HttpError::NotFound);
    };

    if folder.owner_id.is_none() && request.grantee_id.is_some() {
        return Err(HttpError::BadRequest(
            "Folder is already shared with all members".to_string(),
        ));
    }

    if folder.owner_id.is_some_and(|owner_id| owner_id != user.id) {
        return Err(HttpError::NotFound);
    }

    let share = tx
        .create_share(
            folder.id,
            request.grantee_id.as_deref(),
            request.can_upload,
            request.can_delete,
            request.expires_at,
        )
        .await
        .map_err(|e| {
            if is_unique_violation(&e) {
                HttpError::Conflict("This person already has access".to_string())
            } else {
                e.into()
            }
        })?;

    tx.commit().await?;

    info!(
        user = user.id,
        folder_id = folder.id,
        grantee = ?request.grantee_id,
        "Share created"
    );

    let folder_map = HashMap::from([(folder.id, folder.name)]);
    Ok(Json(ShareResponse::from_permission(share, &folder_map)))
}

async fn update_share(
    State(state): State<AppStateRef>,
    AuthenticatedUser(user): AuthenticatedUser,
    Path(share_id): Path<i64>,
    Json(request): Json<UpdateShareRequest>,
) -> HttpResult<impl IntoResponse> {
    // `Some(None)` removes the expiry, `None` (field absent, as sent by Android) keeps it
    let new_expiry = match request.expires_at {
        None => None,
        Some(None) => Some(None),
        Some(Some(unix)) => {
            let expires_at = OffsetDateTime::from_unix_timestamp(unix)
                .map_err(|_| HttpError::BadRequest("Invalid expiry".to_string()))?;
            ensure_in_the_future(expires_at)?;
            Some(Some(expires_at))
        }
    };

    let mut tx = state.write_pool.begin().await?;
    let Some(mut share) = tx
        .as_mut()
        .update_share(share_id, &user.id, request.can_upload, request.can_delete)
        .await?
    else {
        return Err(HttpError::NotFound);
    };

    if let Some(expires_at) = new_expiry {
        share = tx
            .as_mut()
            .set_share_expiry(share_id, &user.id, expires_at)
            .await?
            .ok_or(HttpError::NotFound)?;
    }

    let Some(folder) = tx.get_accessible_folder(&user.id, share.folder_id).await? else {
        return Err(HttpError::NotFound);
    };
    if folder.owner_id.is_some_and(|owner_id| owner_id != user.id) {
        return Err(HttpError::NotFound);
    }

    tx.commit().await?;

    info!(
        user = user.id,
        share_id,
        can_upload = request.can_upload,
        can_delete = request.can_delete,
        "Share updated"
    );

    let folder_map = HashMap::from([(share.folder_id, folder.name)]);
    Ok(Json(ShareResponse::from_permission(share, &folder_map)))
}

async fn revoke_share(
    State(state): State<AppStateRef>,
    AuthenticatedUser(user): AuthenticatedUser,
    Path(share_id): Path<i64>,
) -> HttpResult<impl IntoResponse> {
    let mut tx = state.write_pool.begin().await?;

    let Some(folder) = tx.get_folder_by_share_id(share_id).await? else {
        return Err(HttpError::NotFound);
    };
    if folder.owner_id.is_some_and(|owner_id| owner_id != user.id) {
        return Err(HttpError::NotFound);
    }
    let deleted = tx.delete_share(share_id, &user.id).await?;

    if deleted == 0 {
        return Err(HttpError::NotFound);
    }

    tx.commit().await?;

    info!(user = user.id, share_id, "Share revoked");

    Ok(())
}
