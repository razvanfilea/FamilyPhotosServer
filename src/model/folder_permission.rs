use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::serde::timestamp;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct FolderPermission {
    pub id: i64,
    pub owner_id: String,
    pub folder_name: String,
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
    pub owner_id: String,
    pub folder_name: String,
    pub grantee_id: Option<String>,
    pub token: Option<String>,
    pub can_upload: bool,
    pub can_delete: bool,
    #[serde(with = "timestamp")]
    pub created_at: OffsetDateTime,
    #[serde(with = "timestamp::option")]
    pub expires_at: Option<OffsetDateTime>,
}

impl From<FolderPermission> for ShareResponse {
    fn from(p: FolderPermission) -> Self {
        Self {
            id: p.id,
            owner_id: p.owner_id,
            folder_name: p.folder_name,
            grantee_id: p.grantee_id,
            token: p.token,
            can_upload: p.can_upload,
            can_delete: p.can_delete,
            created_at: p.created_at,
            expires_at: p.expires_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ShareInfo {
    pub folder_name: String,
    pub owner_id: String,
    pub can_upload: bool,
    pub can_delete: bool,
}

impl From<&FolderPermission> for ShareInfo {
    fn from(p: &FolderPermission) -> Self {
        Self {
            folder_name: p.folder_name.clone(),
            owner_id: p.owner_id.clone(),
            can_upload: p.can_upload,
            can_delete: p.can_delete,
        }
    }
}
