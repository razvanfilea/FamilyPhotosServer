use serde::Serialize;
use time::OffsetDateTime;

use time::serde::timestamp;

pub trait PhotoBase {
    fn user_id(&self) -> &str;

    fn name(&self) -> &str;

    fn created_at(&self) -> OffsetDateTime;

    fn file_size(&self) -> i64;

    fn folder_name(&self) -> Option<&str>;

    fn full_name(&self) -> String {
        Self::construct_full_name(self.name(), self.folder_name())
    }

    fn partial_path(&self) -> String {
        format!("{}/{}", self.user_id(), self.full_name())
    }

    fn construct_full_name(name: &str, folder: Option<&str>) -> String {
        if let Some(folder) = folder
            && !folder.is_empty()
        {
            return format!("{folder}/{name}");
        }

        name.to_string()
    }
}

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

impl PhotoBase for Photo {
    fn user_id(&self) -> &str {
        &self.user_id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn created_at(&self) -> OffsetDateTime {
        self.created_at
    }

    fn file_size(&self) -> i64 {
        self.file_size
    }

    fn folder_name(&self) -> Option<&str> {
        self.folder.as_deref()
    }
}

impl Photo {
    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn partial_preview_path(&self) -> String {
        format!("{}.jpg", self.id)
    }
}

#[derive(Debug, Clone)]
pub struct PhotoBody {
    pub user_name: String,
    pub name: String,
    pub created_at: OffsetDateTime,
    pub file_size: i64,
    pub folder: Option<String>,
}

impl PhotoBase for PhotoBody {
    fn user_id(&self) -> &str {
        &self.user_name
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn created_at(&self) -> OffsetDateTime {
        self.created_at
    }

    fn file_size(&self) -> i64 {
        self.file_size
    }

    fn folder_name(&self) -> Option<&str> {
        self.folder.as_deref()
    }
}

impl PhotoBody {
    pub fn new(
        user_name: String,
        name: String,
        created_at: OffsetDateTime,
        file_size: i64,
        folder: Option<String>,
    ) -> Self {
        Self {
            user_name,
            name,
            created_at,
            file_size,
            folder,
        }
    }
    
    pub fn with_id(self, id: i64) -> Photo {
        Photo {
            id,
            user_id: self.user_name,
            name: self.name,
            created_at: self.created_at,
            file_size: self.file_size,
            folder: self.folder,
        }
    }
}
