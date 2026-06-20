use axum::{
    Json, Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::{delete, get, post},
};

use crate::http::AppStateRef;
use crate::http::error::{HttpError, HttpResult};
use crate::http::utils::AuthSession;
use crate::model::folder_permission::{CreateShareRequest, ShareResponse};
use crate::repo::{FolderPermissionsRepo, FoldersRepo, PhotosRepo};

pub fn router() -> Router<AppStateRef> {
    Router::new()
        .route("/", get(list_shares))
        .route("/", post(create_share))
        .route("/{share_id}", delete(revoke_share))
        .route("/{share_id}/photos", get(shared_folder_photos))
        .route("/with-me", get(shared_with_me))
}

async fn list_shares(
    State(state): State<AppStateRef>,
    auth: AuthSession,
) -> HttpResult<impl IntoResponse> {
    let user = auth.user.ok_or(HttpError::Unauthorized)?;

    let shares = state.read_pool.get_shares_by_owner(&user.id).await?;
    let folder_map = state.read_pool.get_folder_name_map().await?;
    let responses: Vec<ShareResponse> = shares
        .into_iter()
        .map(|s| ShareResponse::from_permission(s, &folder_map))
        .collect();

    Ok(Json(responses))
}

async fn create_share(
    State(state): State<AppStateRef>,
    auth: AuthSession,
    Json(request): Json<CreateShareRequest>,
) -> HttpResult<impl IntoResponse> {
    let user = auth.user.ok_or(HttpError::Unauthorized)?;

    let folder = state
        .write_pool
        .get_or_create_folder(Some(&user.id), &request.folder_name)
        .await?;

    let share = state
        .write_pool
        .create_share(
            folder.id,
            request.grantee_id.as_deref(),
            request.can_upload,
            request.can_delete,
            request.expires_at,
        )
        .await?;

    let folder_map = state.read_pool.get_folder_name_map().await?;
    Ok(Json(ShareResponse::from_permission(share, &folder_map)))
}

async fn revoke_share(
    State(state): State<AppStateRef>,
    auth: AuthSession,
    Path(share_id): Path<i64>,
) -> HttpResult<impl IntoResponse> {
    let user = auth.user.ok_or(HttpError::Unauthorized)?;

    let deleted = state.write_pool.delete_share(share_id, &user.id).await?;

    if deleted == 0 {
        return Err(HttpError::NotFound);
    }

    Ok(())
}

async fn shared_folder_photos(
    State(state): State<AppStateRef>,
    auth: AuthSession,
    Path(share_id): Path<i64>,
) -> HttpResult<impl IntoResponse> {
    let user = auth.user.ok_or(HttpError::Unauthorized)?;

    let permission = state
        .read_pool
        .get_permission_by_id(share_id)
        .await?
        .ok_or(HttpError::NotFound)?;

    if permission.grantee_id.as_deref() != Some(&user.id) {
        return Err(HttpError::NotFound);
    }

    if permission.is_expired() {
        return Err(HttpError::NotFound);
    }

    let photos = state
        .read_pool
        .get_photos_in_folder(permission.folder_id)
        .await?;

    Ok(Json(photos))
}

async fn shared_with_me(
    State(state): State<AppStateRef>,
    auth: AuthSession,
) -> HttpResult<impl IntoResponse> {
    let user = auth.user.ok_or(HttpError::Unauthorized)?;

    let shares = state.read_pool.get_shares_for_grantee(&user.id).await?;
    let folder_map = state.read_pool.get_folder_name_map().await?;
    let responses: Vec<ShareResponse> = shares
        .into_iter()
        .map(|s| ShareResponse::from_permission(s, &folder_map))
        .collect();

    Ok(Json(responses))
}
