use crate::http::AppStateRef;
use crate::http::error::HttpResult;
use crate::http::page_context::{Nav, PageContext};
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::views::{DateStyle, format_bytes, format_date};
use crate::model::photo::Photo;
use crate::repo::PhotosHashRepo;
use crate::utils::media::is_video;
use askama::Template;
use axum::extract::State;
use axum::response::Response;
use tracing::error;

/// A button of "Which do you want to keep?"
struct CopyView {
    id: i64,
    name: String,
    /// Public ("Family") or private ("Personal")
    type_icon: &'static str,
    type_label: &'static str,
    /// `None` for photos outside folders
    folder_name: Option<String>,
    date: String,
    size: String,
}

/// Photos with the same content
struct GroupView {
    copies: Vec<CopyView>,
    /// The large image: the first copy's screen rendition (videos only have a preview)
    image_url: String,
    /// Shown when the large image fails to load
    fallback_url: String,
}

impl GroupView {
    fn new(photos: &[(Photo, Option<String>)]) -> Self {
        let first = &photos[0].0;
        let preview_url = format!("/api/preview/{}", first.id);

        Self {
            image_url: if is_video(&first.name) {
                preview_url.clone()
            } else {
                format!("/api/screen/{}", first.id)
            },
            fallback_url: preview_url,
            copies: photos
                .iter()
                .map(|(photo, folder_name)| {
                    let public = photo.user_id.is_none();
                    CopyView {
                        id: photo.id,
                        name: photo.name.clone(),
                        type_icon: if public { "family_restroom" } else { "person" },
                        type_label: if public { "Family" } else { "Personal" },
                        folder_name: folder_name.clone(),
                        date: format_date(photo.created_at, DateStyle::Day),
                        size: format_bytes(photo.file_size),
                    }
                })
                .collect(),
        }
    }
}

#[derive(Template)]
#[template(path = "duplicates/duplicates_page.html")]
struct DuplicatesTemplate {
    ctx: PageContext,
    /// Loading the groups failed
    failed: bool,
    groups: Vec<GroupView>,
}

/// `GET /duplicates`: one group of identical photos at a time (switched in the browser)
pub async fn duplicates_page(
    State(state): State<AppStateRef>,
    ctx: PageContext,
) -> HttpResult<Response> {
    let (failed, groups) = match state
        .read_pool
        .get_duplicate_photos_for_user(&ctx.user_id)
        .await
    {
        Ok(groups) => (
            false,
            groups.iter().map(|group| GroupView::new(group)).collect(),
        ),
        Err(e) => {
            error!("Failed to load the duplicates: {e}");
            (true, Vec::new())
        }
    };

    DuplicatesTemplate {
        ctx: ctx.nav(Nav::Utilities),
        failed,
        groups,
    }
    .try_into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::api::tests::TestServer;
    use crate::model::photo::Photo;
    use crate::model::photo_hash::PhotoHash;
    use crate::repo::tests::{create_test_folder, insert_photo, insert_users, photo};
    use axum::http::StatusCode;
    use sqlx::SqlitePool;
    use time::OffsetDateTime;

    async fn hashed(pool: &SqlitePool, source: Photo, hash: u8) -> Photo {
        let inserted = insert_photo(pool, source).await;
        pool.insert_hash(PhotoHash {
            id: inserted.id,
            hash: vec![hash; 16],
        })
        .await
        .unwrap();
        inserted
    }

    #[sqlx::test]
    async fn test_get_duplicate_photos_for_user(pool: SqlitePool) -> sqlx::Result<()> {
        insert_users(&pool).await;
        let folder = create_test_folder(&pool, Some("alice"), "mine").await;

        let a = hashed(&pool, photo(Some("alice"), "a"), 1).await;
        let b = hashed(
            &pool,
            Photo {
                folder_id: Some(folder.id),
                ..photo(Some("alice"), "b")
            },
            1,
        )
        .await;
        let public = hashed(&pool, photo(None, "public"), 1).await;

        // A group that is no longer one once a copy is trashed
        hashed(&pool, photo(Some("alice"), "c"), 2).await;
        hashed(
            &pool,
            Photo {
                trashed_on: Some(OffsetDateTime::now_utc()),
                ..photo(Some("alice"), "c-trashed")
            },
            2,
        )
        .await;

        // Other people's duplicates, and a single photo
        hashed(&pool, photo(Some("bob"), "bob-1"), 3).await;
        hashed(&pool, photo(Some("bob"), "bob-2"), 3).await;
        hashed(&pool, photo(Some("alice"), "single"), 4).await;

        let groups = pool.get_duplicate_photos_for_user("alice").await?;
        let summary: Vec<Vec<(i64, Option<&str>)>> = groups
            .iter()
            .map(|group| {
                group
                    .iter()
                    .map(|(photo, folder)| (photo.id, folder.as_deref()))
                    .collect()
            })
            .collect();
        assert_eq!(
            summary,
            vec![vec![(a.id, None), (b.id, Some("mine")), (public.id, None)]]
        );

        assert!(
            pool.get_duplicate_photos_for_user("carol")
                .await?
                .is_empty()
        );
        Ok(())
    }

    #[sqlx::test]
    async fn test_duplicates_page(pool: SqlitePool) {
        let server = TestServer::new(pool).await;
        let pool = server.pool();
        let first = hashed(pool, photo(Some("alice"), "first.jpg"), 1).await;
        let second = hashed(pool, photo(None, "second.mp4"), 1).await;

        let html = server
            .login("alice")
            .await
            .get("/duplicates")
            .await
            .assert_status(StatusCode::OK)
            .text();
        assert!(html.contains(&format!("/api/screen/{}", first.id)));
        assert_eq!(html.matches("data-testid=\"dup-keep\"").count(), 2);
        assert!(html.contains(&format!("data-photo-id=\"{}\"", second.id)));
        assert!(html.contains("No folder"));
        assert!(!html.contains("Failed to load duplicates"));

        let html = server
            .login("bob")
            .await
            .get("/duplicates")
            .await
            .assert_status(StatusCode::OK)
            .text();
        assert!(html.contains("No duplicates found"));
        assert!(!html.contains("dup-keep"));
    }

    #[test]
    fn test_failed_page() {
        let user = crate::repo::tests::create_test_user("alice", "Alice");
        let html = DuplicatesTemplate {
            ctx: PageContext::new(&user, Default::default()),
            failed: true,
            groups: Vec::new(),
        }
        .render()
        .unwrap();
        assert!(html.contains("Failed to load duplicates"));
        assert!(!html.contains("No duplicates found"));
    }
}
