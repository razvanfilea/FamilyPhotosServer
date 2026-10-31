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
    pub folder_id: Option<i64>,
    #[serde(with = "crate::utils::serde_helpers::base64_option")]
    pub thumb_hash: Option<Vec<u8>>,
    #[serde(with = "timestamp::option")]
    pub trashed_on: Option<OffsetDateTime>,
}

impl Photo {
    pub fn full_name(&self, folder_name: Option<&str>) -> String {
        Self::construct_full_name(&self.name, folder_name)
    }

    pub fn partial_path(&self, folder_name: Option<&str>) -> String {
        format!(
            "{}/{}",
            self.user_id.as_deref().unwrap_or(PUBLIC_USER_FOLDER),
            self.full_name(folder_name)
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
        format!("{}.webp", photo_id)
    }
}

pub struct PhotoWithFolder {
    pub photo: Photo,
    pub folder_name: Option<String>,
}

impl PhotoWithFolder {
    pub fn partial_path(&self) -> String {
        self.photo.partial_path(self.folder_name.as_deref())
    }
}

#[derive(Serialize)]
pub struct FullPhotosList {
    pub event_log_id: i64,
    pub photos: Vec<Photo>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::tests::photo;

    #[test]
    fn test_names_and_paths() {
        // (owner, folder, full name, partial path)
        let cases = [
            (
                Some("alice"),
                Some("vacation"),
                "vacation/beach.jpg",
                "alice/vacation/beach.jpg",
            ),
            (Some("alice"), Some(""), "beach.jpg", "alice/beach.jpg"),
            (Some("alice"), None, "beach.jpg", "alice/beach.jpg"),
            (None, None, "beach.jpg", "public/beach.jpg"),
            (
                None,
                Some("family"),
                "family/beach.jpg",
                "public/family/beach.jpg",
            ),
        ];
        for (owner, folder, full_name, partial_path) in cases {
            let photo = photo(owner, "beach.jpg");
            assert_eq!(photo.full_name(folder), full_name, "{owner:?} {folder:?}");
            assert_eq!(
                photo.partial_path(folder),
                partial_path,
                "{owner:?} {folder:?}"
            );
        }

        for (id, path) in [(123, "123.webp"), (0, "0.webp")] {
            assert_eq!(Photo::construct_partial_preview_path(id), path);
        }
    }
}
