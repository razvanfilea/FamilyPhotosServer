use time::OffsetDateTime;

#[derive(Debug, Clone)]
pub struct Folder {
    pub id: i64,
    pub owner_id: Option<String>,
    pub name: String,
    #[allow(dead_code)]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct AccessibleFolder {
    pub id: i64,
    pub owner_id: Option<String>,
    pub name: String,
    pub can_upload: bool,
    pub can_delete: bool,
}
