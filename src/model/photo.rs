use serde::Serialize;
use time::OffsetDateTime;

use time::serde::timestamp;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Photo {
    pub id: i64,
    pub user_id: String,
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
        format!("{}/{}", self.user_id, self.full_name())
    }

    pub fn partial_preview_path(&self) -> String {
        format!("{}.jpg", self.id)
    }

    pub(crate) fn construct_full_name(name: &str, folder: Option<&str>) -> String {
        if let Some(folder) = folder
            && !folder.is_empty()
        {
            return format!("{folder}/{name}");
        }

        name.to_string()
    }
}
