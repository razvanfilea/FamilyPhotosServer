use serde::Serialize;
use serde_with::serde_as;
use time::OffsetDateTime;

use crate::model::user::PUBLIC_USER_FOLDER;
use time::serde::timestamp;

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
pub struct Photo {
    pub id: i64,
    pub user_id: Option<String>,
    pub name: String,
    #[serde(with = "timestamp")]
    pub created_at: OffsetDateTime,
    pub file_size: i64,
    pub folder_id: Option<i64>,
    #[serde_as(as = "Option<serde_with::base64::Base64>")]
    pub thumb_hash: Option<Vec<u8>>,
    #[serde(with = "timestamp::option")]
    pub trashed_on: Option<OffsetDateTime>,
}

impl Photo {
    pub fn id(&self) -> i64 {
        self.id
    }

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

    pub fn partial_preview_path(&self) -> String {
        self.photo.partial_preview_path()
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

    fn test_photo(user_id: Option<&str>, name: &str) -> Photo {
        Photo {
            id: 42,
            user_id: user_id.map(String::from),
            name: name.to_string(),
            created_at: OffsetDateTime::UNIX_EPOCH,
            file_size: 1024,
            folder_id: None,
            thumb_hash: None,
            trashed_on: None,
        }
    }

    #[test]
    fn test_construct_full_name() {
        assert_eq!(
            Photo::construct_full_name("beach.jpg", Some("vacation")),
            "vacation/beach.jpg"
        );
        assert_eq!(Photo::construct_full_name("beach.jpg", None), "beach.jpg");
        assert_eq!(
            Photo::construct_full_name("beach.jpg", Some("")),
            "beach.jpg"
        );
    }

    #[test]
    fn test_construct_partial_preview_path() {
        assert_eq!(Photo::construct_partial_preview_path(123), "123.webp");
        assert_eq!(Photo::construct_partial_preview_path(0), "0.webp");
    }

    #[test]
    fn test_partial_path_with_folder() {
        let photo = test_photo(Some("alice"), "beach.jpg");
        assert_eq!(
            photo.partial_path(Some("vacation")),
            "alice/vacation/beach.jpg"
        );
    }

    #[test]
    fn test_partial_path_without_folder() {
        let photo = test_photo(Some("alice"), "beach.jpg");
        assert_eq!(photo.partial_path(None), "alice/beach.jpg");

        let public_photo = test_photo(None, "shared.jpg");
        assert_eq!(public_photo.partial_path(None), "public/shared.jpg");
    }
}
