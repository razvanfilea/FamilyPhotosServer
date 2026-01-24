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
use crate::repo::FolderPermissionsRepo;

pub fn router() -> Router<AppStateRef> {
    Router::new()
        .route("/", get(list_shares))
        .route("/", post(create_share))
        .route("/{share_id}", delete(revoke_share))
        .route("/shared-with-me", get(shared_with_me))
}

async fn list_shares(
    State(state): State<AppStateRef>,
    auth: AuthSession,
) -> HttpResult<impl IntoResponse> {
    let user = auth.user.ok_or(HttpError::Unauthorized)?;

    let shares = state.pool.get_shares_by_owner(&user.id).await?;
    let responses: Vec<ShareResponse> = shares.into_iter().map(ShareResponse::from).collect();

    Ok(Json(responses))
}

async fn create_share(
    State(state): State<AppStateRef>,
    auth: AuthSession,
    Json(request): Json<CreateShareRequest>,
) -> HttpResult<impl IntoResponse> {
    let user = auth.user.ok_or(HttpError::Unauthorized)?;

    let share = state
        .pool
        .create_share(
            &user.id,
            &request.folder_name,
            request.grantee_id.as_deref(),
            request.can_upload,
            request.can_delete,
            request.expires_at,
        )
        .await?;

    Ok(Json(ShareResponse::from(share)))
}

async fn revoke_share(
    State(state): State<AppStateRef>,
    auth: AuthSession,
    Path(share_id): Path<i64>,
) -> HttpResult<impl IntoResponse> {
    let user = auth.user.ok_or(HttpError::Unauthorized)?;

    let deleted = state.pool.delete_share(share_id, &user.id).await?;

    if deleted == 0 {
        return Err(HttpError::NotFound);
    }

    Ok(())
}

async fn shared_with_me(
    State(state): State<AppStateRef>,
    auth: AuthSession,
) -> HttpResult<impl IntoResponse> {
    let user = auth.user.ok_or(HttpError::Unauthorized)?;

    let shares = state.pool.get_shares_for_grantee(&user.id).await?;
    let responses: Vec<ShareResponse> = shares.into_iter().map(ShareResponse::from).collect();

    Ok(Json(responses))
}
