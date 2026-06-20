use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use time::OffsetDateTime;
use time::serde::timestamp;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct FolderPermission {
    pub id: i64,
    pub folder_id: i64,
    pub grantee_id: Option<String>,
    pub token: Option<String>,
    pub can_upload: bool,
    pub can_delete: bool,
    pub created_at: OffsetDateTime,
    pub expires_at: Option<OffsetDateTime>,
}

impl FolderPermission {
    pub fn is_expired(&self) -> bool {
        self.expires_at
            .map(|expires| expires < OffsetDateTime::now_utc())
            .unwrap_or(false)
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateShareRequest {
    pub folder_name: String,
    pub grantee_id: Option<String>,
    #[serde(default)]
    pub can_upload: bool,
    #[serde(default)]
    pub can_delete: bool,
    #[serde(default, with = "timestamp::option")]
    pub expires_at: Option<OffsetDateTime>,
}

#[derive(Debug, Serialize)]
pub struct ShareResponse {
    pub id: i64,
    pub folder_id: i64,
    pub folder_name: Option<String>,
    pub grantee_id: Option<String>,
    pub token: Option<String>,
    pub can_upload: bool,
    pub can_delete: bool,
    #[serde(with = "timestamp")]
    pub created_at: OffsetDateTime,
    #[serde(with = "timestamp::option")]
    pub expires_at: Option<OffsetDateTime>,
}

impl ShareResponse {
    pub fn from_permission(p: FolderPermission, folder_map: &HashMap<i64, String>) -> Self {
        let folder_name = folder_map.get(&p.folder_id).cloned();
        Self {
            id: p.id,
            folder_id: p.folder_id,
            folder_name,
            grantee_id: p.grantee_id,
            token: p.token,
            can_upload: p.can_upload,
            can_delete: p.can_delete,
            created_at: p.created_at,
            expires_at: p.expires_at,
        }
    }
}
