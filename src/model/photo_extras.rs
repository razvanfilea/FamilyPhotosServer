use crate::model::exif_field::ExifFields;

pub struct PhotoExtras {
    pub id: i64,
    pub hash: Vec<u8>,
    pub exif_json: Option<sqlx::types::Json<ExifFields>>,
}