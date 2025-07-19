use serde::Serialize;
use time::OffsetDateTime;

use crate::model::user::PUBLIC_USER_FOLDER;
use time::serde::timestamp;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
pub struct Photo {
    pub id: i64,
    pub user_id: Option<String>,
    pub name: String,
    #[serde(with = "timestamp")]
    pub created_at: OffsetDateTime,
    pub file_size: i64,
    pub folder: Option<String>,
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
        format!("{}.jpg", self.id)
    }

    pub fn construct_full_name(name: &str, folder: Option<&str>) -> String {
        if let Some(folder) = folder
            && !folder.is_empty()
        {
            return format!("{folder}/{name}");
        }

        name.to_string()
    }
}

#[derive(Serialize)]
pub struct FullPhotosList {
    pub event_log_id: i64,
    pub photos: Vec<Photo>,
}
