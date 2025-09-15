use serde::Serialize;
use serde_with::serde_as;
use time::OffsetDateTime;

use crate::model::user::PUBLIC_USER_FOLDER;
use time::serde::timestamp;

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
pub struct Photo {
    pub id: i64,
    pub user_id: Option<String>,
    pub name: String,
    #[serde(with = "timestamp")]
    pub created_at: OffsetDateTime,
    pub file_size: i64,
    pub folder: Option<String>,
    #[serde_as(as = "Option<serde_with::base64::Base64>")]
    pub thumb_hash: Option<Vec<u8>>,
    #[serde(with = "timestamp::option")]
    pub trashed_on: Option<OffsetDateTime>,
}

impl Photo {
    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn full_name(&self) -> String {
        Self::construct_full_name(&self.name, self.folder.as_deref())
    }

    pub fn partial_path(&self) -> String {
        format!(
            "{}/{}",
            self.user_id.as_deref().unwrap_or(PUBLIC_USER_FOLDER),
            self.full_name()
        )
    }

    pub fn partial_preview_path(&self) -> String {
        Self::construct_partial_preview_path(self.id)
    }

    pub fn construct_full_name(name: &str, folder: Option<&str>) -> String {
        if let Some(folder) = folder
            && !folder.is_empty()
        {
            return format!("{folder}/{name}");
        }

        name.to_string()
    }

    pub fn construct_partial_preview_path(photo_id: i64) -> String {
        format!("{}.jpg", photo_id)
    }
}

#[derive(Serialize)]
pub struct FullPhotosList {
    pub event_log_id: i64,
    pub photos: Vec<Photo>,
}
