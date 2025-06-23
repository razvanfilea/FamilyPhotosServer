use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ExifField {
    pub tag: String,
    pub value: String,
}

pub type ExifFields = Vec<ExifField>;

