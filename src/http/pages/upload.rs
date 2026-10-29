//! The upload page (`assets/js/upload`): a destination, then files that are hashed in the browser,
//! checked against the server and uploaded through `/api/upload`.

use crate::http::AppStateRef;
use crate::http::error::HttpResult;
use crate::http::page_context::{Nav, PageContext};
use crate::http::pages::folders::folder_type_info;
use crate::http::template_into_response::TemplateIntoResponse;
use crate::repo::FoldersRepo;
use askama::Template;
use axum::extract::{Query, State};
use axum::response::Response;
use serde::Deserialize;
use sqlx::SqliteExecutor;

/// Larger files can't be uploaded: the server's request body limit (`DefaultBodyLimit`) is 1 GiB,
/// and the multipart framing needs a few bytes of it.
const MAX_FILE_SIZE: u64 = 1024 * 1024 * 1024 - 64 * 1024;

/// The folder an upload page opened with `?folder_id=` goes to
#[derive(Debug, PartialEq)]
struct InitialFolder {
    id: i64,
    name: String,
    type_label: &'static str,
}

#[derive(Template)]
#[template(path = "upload/upload_page.html")]
struct UploadPageTemplate {
    ctx: PageContext,
    initial_folder: Option<InitialFolder>,
    max_file_size: u64,
}

impl UploadPageTemplate {
    /// The URL of the destination picker fragment
    fn picker_url(&self) -> String {
        match &self.initial_folder {
            Some(folder) => format!("/ui/destination-picker?mode=upload&folder_id={}", folder.id),
            None => "/ui/destination-picker?mode=upload".to_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct UploadPageQuery {
    /// Kept as text: a malformed id just preselects nothing
    folder_id: Option<String>,
}

/// The folder to preselect, if the user can upload to it
async fn initial_folder<'c, E: SqliteExecutor<'c>>(
    executor: E,
    user_id: &str,
    folder_id: Option<&str>,
) -> sqlx::Result<Option<InitialFolder>> {
    let Some(folder_id) = folder_id.and_then(|id| id.parse::<i64>().ok()) else {
        return Ok(None);
    };

    let card = executor.get_folder_card(user_id, folder_id).await?;
    Ok(card
        .filter(|card| card.can_upload)
        .map(|card| InitialFolder {
            id: card.id,
            name: card.name,
            type_label: folder_type_info(card.folder_type).1,
        }))
}

/// `GET /upload?folder_id=`
pub async fn upload_page(
    ctx: PageContext,
    State(state): State<AppStateRef>,
    Query(query): Query<UploadPageQuery>,
) -> HttpResult<Response> {
    let initial_folder =
        initial_folder(&state.read_pool, &ctx.user_id, query.folder_id.as_deref()).await?;

    UploadPageTemplate {
        ctx: ctx.nav(Nav::Upload),
        initial_folder,
        max_file_size: MAX_FILE_SIZE,
    }
    .try_into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::tests::{Dataset, insert_users};
    use sqlx::SqlitePool;

    #[sqlx::test]
    async fn test_initial_folder(pool: SqlitePool) {
        insert_users(&pool).await;
        let data = Dataset::create(&pool).await;
        let id = |id: i64| Some(id.to_string());

        let cases = [
            (id(data.own_folder), Some(("mine", "Personal"))),
            (
                id(data.shared_delete_folder),
                Some(("shared-delete", "Shared")),
            ),
            // View only, expired, not shared, missing
            (id(data.shared_folder), None),
            (id(data.expired_folder), None),
            (id(data.unshared_folder), None),
            (id(i64::MAX), None),
            (Some("abc".to_string()), None),
            (None, None),
        ];

        for (folder_id, expected) in cases {
            let folder = initial_folder(&pool, "alice", folder_id.as_deref())
                .await
                .unwrap();
            let expected = expected.map(|(name, type_label)| InitialFolder {
                id: folder_id.as_deref().unwrap().parse().unwrap(),
                name: name.to_string(),
                type_label,
            });
            assert_eq!(folder, expected, "{folder_id:?}");
        }
    }
}
