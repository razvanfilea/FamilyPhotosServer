use crate::model::exif_field::ExifFields;

pub struct PhotoExtras {
    pub id: i64,
    pub sha: String,
    pub exif_json: Option<sqlx::types::Json<ExifFields>>,
}