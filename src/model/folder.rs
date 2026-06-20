use serde::Serialize;
use time::OffsetDateTime;

#[derive(Debug, Clone, Serialize)]
pub struct Folder {
    pub id: i64,
    pub owner_id: Option<String>,
    pub name: String,
    #[serde(with = "timestamp")]
    pub created_at: OffsetDateTime,
}
