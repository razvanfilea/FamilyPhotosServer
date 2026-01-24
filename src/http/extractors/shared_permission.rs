use crate::http::AppStateRef;
use crate::http::error::HttpError;
use crate::model::folder_permission::FolderPermission;
use crate::repo::FolderPermissionsRepo;
use axum::extract::{FromRequestParts, Path};
use axum::http::request::Parts;
use std::collections::HashMap;

pub struct SharedPermission(pub FolderPermission);

impl FromRequestParts<AppStateRef> for SharedPermission {
    type Rejection = HttpError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppStateRef,
    ) -> Result<Self, Self::Rejection> {
        let path_params: Option<HashMap<String, String>> = Path::from_request_parts(parts, state)
            .await
            .map(|path| path.0)
            .ok();

        let token = path_params
            .as_ref()
            .and_then(|path| path.get("token"))
            .ok_or_else(|| HttpError::BadRequest("Missing token in path".to_string()))?;

        let permission = state
            .pool
            .get_permission_by_token(token)
            .await?
            .ok_or(HttpError::NotFound)?;

        if permission.is_expired() {
            return Err(HttpError::NotFound);
        }

        Ok(SharedPermission(permission))
    }
}
