use crate::http::AppStateRef;
use crate::http::auth::AuthenticatedUser;
use crate::http::error::{HttpError, HttpResult};
use crate::model::photo_filter::PhotoFilter;
use crate::repo::FoldersRepo;
use axum::extract::{FromRequestParts, Query};
use axum::http::request::Parts;
use serde::Deserialize;
use sqlx::SqliteExecutor;
use std::fmt;
use std::str::FromStr;

pub const MIN_MEMORIES_YEARS_AGO: u32 = 1;
pub const MAX_MEMORIES_YEARS_AGO: u32 = 10;

/// An ordered list of photos that a grid or the viewer operates on.
///
/// Serialized as a short `contextKey` string (see [`PhotoListContext::key`]) that the UI passes
/// back in `?ctx=` so the server can recompute the exact same list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhotoListContext {
    Timeline(PhotoFilter),
    Folder(i64),
    Favorites,
    Trash,
    LargeFiles,
    /// "On this day", `years_ago` years ago, restricted by the timeline filter
    Memories {
        years_ago: u32,
        filter: PhotoFilter,
    },
}

impl PhotoListContext {
    /// The `contextKey`: `timeline:all`, `folder:12`, `favorites`, `trash`, `large-files`,
    /// `memories:3` (filter `all`) or `memories:3:personal`.
    #[allow(dead_code)] // TODO: used by the grid pages
    pub fn key(&self) -> String {
        self.to_string()
    }

    /// Whether the context is displayed as month sections (and month queries apply).
    #[allow(dead_code)] // TODO: used by the grid pages
    pub fn is_month_sectioned(&self) -> bool {
        matches!(
            self,
            PhotoListContext::Timeline(_)
                | PhotoListContext::Folder(_)
                | PhotoListContext::Favorites
        )
    }

    /// Checks that the user may see this context at all. Per-photo access is part of the
    /// context queries themselves.
    pub async fn authorize<'c, E>(&self, user_id: &str, executor: E) -> HttpResult<()>
    where
        E: SqliteExecutor<'c>,
    {
        if let PhotoListContext::Folder(folder_id) = *self {
            executor
                .get_accessible_folder(user_id, folder_id)
                .await?
                .ok_or(HttpError::NotFound)?;
        }
        Ok(())
    }
}

impl fmt::Display for PhotoListContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PhotoListContext::Timeline(filter) => write!(f, "timeline:{filter}"),
            PhotoListContext::Folder(id) => write!(f, "folder:{id}"),
            PhotoListContext::Favorites => f.write_str("favorites"),
            PhotoListContext::Trash => f.write_str("trash"),
            PhotoListContext::LargeFiles => f.write_str("large-files"),
            PhotoListContext::Memories {
                years_ago,
                filter: PhotoFilter::All,
            } => write!(f, "memories:{years_ago}"),
            PhotoListContext::Memories { years_ago, filter } => {
                write!(f, "memories:{years_ago}:{filter}")
            }
        }
    }
}

impl FromStr for PhotoListContext {
    type Err = HttpError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || HttpError::BadRequest(format!("Invalid context: {s}"));

        let mut parts = s.split(':');
        let kind = parts.next().unwrap_or_default();
        let arg = parts.next();
        let extra = parts.next();
        if parts.next().is_some() {
            return Err(invalid());
        }

        let context = match (kind, arg, extra) {
            ("timeline", Some(filter), None) => {
                PhotoListContext::Timeline(filter.parse().map_err(|_| invalid())?)
            }
            ("folder", Some(id), None) => {
                PhotoListContext::Folder(id.parse().map_err(|_| invalid())?)
            }
            ("favorites", None, None) => PhotoListContext::Favorites,
            ("trash", None, None) => PhotoListContext::Trash,
            ("large-files", None, None) => PhotoListContext::LargeFiles,
            ("memories", Some(years_ago), filter) => {
                let years_ago: u32 = years_ago.parse().map_err(|_| invalid())?;
                if !(MIN_MEMORIES_YEARS_AGO..=MAX_MEMORIES_YEARS_AGO).contains(&years_ago) {
                    return Err(invalid());
                }
                let filter = match filter {
                    Some(filter) => filter.parse().map_err(|_| invalid())?,
                    None => PhotoFilter::All,
                };
                PhotoListContext::Memories { years_ago, filter }
            }
            _ => return Err(invalid()),
        };

        Ok(context)
    }
}

/// Whether `month` has the `YYYY-MM` format used by month sections
pub fn is_valid_month(month: &str) -> bool {
    let bytes = month.as_bytes();
    bytes.len() == 7
        && bytes[4] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || b.is_ascii_digit())
}

#[derive(Deserialize)]
struct ContextParam {
    ctx: String,
}

/// Extracts `?ctx=<contextKey>`, parses it (400 if invalid) and authorizes it for the logged-in
/// user (401 if logged out, 404 if e.g. the folder isn't accessible).
pub struct ContextQuery(pub PhotoListContext);

impl FromRequestParts<AppStateRef> for ContextQuery {
    type Rejection = HttpError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppStateRef,
    ) -> Result<Self, Self::Rejection> {
        let AuthenticatedUser(user) = AuthenticatedUser::from_request_parts(parts, state).await?;

        let Query(param) = Query::<ContextParam>::from_request_parts(parts, state)
            .await
            .map_err(|_| HttpError::BadRequest("Missing context".to_string()))?;

        let context: PhotoListContext = param.ctx.parse()?;
        context.authorize(&user.id, &state.read_pool).await?;

        Ok(ContextQuery(context))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_round_trip() {
        let contexts = [
            PhotoListContext::Timeline(PhotoFilter::All),
            PhotoListContext::Timeline(PhotoFilter::Personal),
            PhotoListContext::Timeline(PhotoFilter::Family),
            PhotoListContext::Timeline(PhotoFilter::Shared),
            PhotoListContext::Folder(12),
            PhotoListContext::Favorites,
            PhotoListContext::Trash,
            PhotoListContext::LargeFiles,
            PhotoListContext::Memories {
                years_ago: 3,
                filter: PhotoFilter::All,
            },
            PhotoListContext::Memories {
                years_ago: 10,
                filter: PhotoFilter::Family,
            },
        ];

        for context in contexts {
            assert_eq!(context.key().parse::<PhotoListContext>().unwrap(), context);
        }

        assert_eq!(PhotoListContext::Folder(12).key(), "folder:12");
        assert_eq!(
            PhotoListContext::Memories {
                years_ago: 3,
                filter: PhotoFilter::All
            }
            .key(),
            "memories:3"
        );
        assert_eq!(PhotoListContext::LargeFiles.key(), "large-files");
    }

    #[test]
    fn test_invalid_keys() {
        for key in [
            "",
            "timeline",
            "timeline:",
            "timeline:everything",
            "timeline:all:x",
            "folder",
            "folder:abc",
            "favorites:1",
            "trash:",
            "memories",
            "memories:0",
            "memories:11",
            "memories:-1",
            "memories:3:nope",
            "memories:3:all:x",
            "unknown",
        ] {
            assert!(
                matches!(
                    key.parse::<PhotoListContext>(),
                    Err(HttpError::BadRequest(_))
                ),
                "{key} should be invalid"
            );
        }
    }

    #[test]
    fn test_is_valid_month() {
        assert!(is_valid_month("2024-06"));
        assert!(is_valid_month("1999-12"));
        for month in [
            "2024-6",
            "2024/06",
            "24-06",
            "2024-06-01",
            "abcd-ef",
            "",
            "2024-0a",
        ] {
            assert!(!is_valid_month(month), "{month}");
        }
    }

    #[test]
    fn test_is_month_sectioned() {
        assert!(PhotoListContext::Timeline(PhotoFilter::Shared).is_month_sectioned());
        assert!(PhotoListContext::Folder(1).is_month_sectioned());
        assert!(PhotoListContext::Favorites.is_month_sectioned());
        assert!(!PhotoListContext::Trash.is_month_sectioned());
        assert!(!PhotoListContext::LargeFiles.is_month_sectioned());
        assert!(
            !PhotoListContext::Memories {
                years_ago: 1,
                filter: PhotoFilter::All
            }
            .is_month_sectioned()
        );
    }
}
