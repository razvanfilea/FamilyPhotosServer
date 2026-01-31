use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PhotoCategory {
    #[default]
    All,
    Personal,
    Family,
}

impl fmt::Display for PhotoCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PhotoCategory::All => write!(f, "all"),
            PhotoCategory::Personal => write!(f, "personal"),
            PhotoCategory::Family => write!(f, "family"),
        }
    }
}
