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
    /// The copy to keep unless the user decides otherwise
    suggested: bool,
}

/// A bulk action that applies to a group, see [`bulk_kind`]
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum BulkKind {
    /// A Personal and a Family copy: the Personal one goes
    KeepFamily,
    /// Same owner and folder: only the suggested copy stays
    RemoveExtra,
}

impl BulkKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::KeepFamily => "family",
            Self::RemoveExtra => "extra",
        }
    }
}

type Copies = [(Photo, Option<String>)];

/// The copy to keep: the Family copy, then a copy in a folder, then the lowest id (the oldest upload)
fn suggested_index(photos: &Copies) -> usize {
    photos
        .iter()
        .enumerate()
        .min_by_key(|(_, (photo, _))| {
            (photo.user_id.is_some(), photo.folder_id.is_none(), photo.id)
        })
        .map_or(0, |(index, _)| index)
}

/// The bulk action that resolves the group, if any. Both keep the suggested copy.
fn bulk_kind(photos: &Copies) -> Option<BulkKind> {
    let public = photos.iter().filter(|(p, _)| p.user_id.is_none()).count();
    if photos.len() == 2 && public == 1 {
        return Some(BulkKind::KeepFamily);
    }

    let (first, _) = photos.first()?;
    let same_place = photos
        .iter()
        .all(|(p, _)| p.user_id == first.user_id && p.folder_id == first.folder_id);
    (photos.len() >= 2 && same_place).then_some(BulkKind::RemoveExtra)
}

/// Photos with the same content
struct GroupView {
    copies: Vec<CopyView>,
    /// The large image: the first copy's screen rendition (videos only have a preview)
    image_url: String,
    /// Shown when the large image fails to load
    fallback_url: String,
    /// The bulk action for this group ("family", "extra"), empty when there is none
    bulk: &'static str,
    /// The size of the copies that would go to the trash: all but the suggested one
    freed_bytes: i64,
    freed: String,
}

impl GroupView {
    fn new(photos: &Copies) -> Self {
        let first = &photos[0].0;
        let preview_url = format!("/api/preview/{}", first.id);
        let suggested = suggested_index(photos);
        let freed_bytes = photos
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != suggested)
            .map(|(_, (photo, _))| photo.file_size)
            .sum();

        Self {
            image_url: if is_video(&first.name) {
                preview_url.clone()
            } else {
                format!("/api/screen/{}", first.id)
            },
            fallback_url: preview_url,
            bulk: bulk_kind(photos).map_or("", BulkKind::as_str),
            freed_bytes,
            freed: format_bytes(freed_bytes),
            copies: photos
                .iter()
                .enumerate()
                .map(|(index, (photo, folder_name))| {
                    let public = photo.user_id.is_none();
                    CopyView {
                        id: photo.id,
                        name: photo.name.clone(),
                        type_icon: if public { "family_restroom" } else { "person" },
                        type_label: if public { "Family" } else { "Personal" },
                        folder_name: folder_name.clone(),
                        date: format_date(photo.created_at, DateStyle::Day),
                        suggested: index == suggested,
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
        // The Family copy is the suggested one, and the file names are shown
        assert_eq!(html.matches("data-testid=\"dup-suggested\"").count(), 1);
        assert!(html.contains(&format!(
            "data-photo-id=\"{}\" title=\"second.mp4\" data-suggested",
            second.id
        )));
        assert!(html.contains(">first.jpg</span>") && html.contains(">second.mp4</span>"));
        // Personal + Family pair: the Personal copy can be trashed in bulk
        assert!(html.contains("data-bulk=\"family\""));
        assert!(html.contains("Frees "));
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

    fn copy(id: i64, user_id: Option<&str>, folder_id: Option<i64>) -> (Photo, Option<String>) {
        (
            Photo {
                id,
                folder_id,
                file_size: 100 * id,
                ..photo(user_id, "copy.jpg")
            },
            None,
        )
    }

    #[test]
    fn test_suggested_copy() {
        // The Family copy, even when the Personal one is in a folder and older
        let group = [copy(1, Some("alice"), Some(5)), copy(2, None, None)];
        assert_eq!(suggested_index(&group), 1);
        // Then a copy in a folder
        let group = [
            copy(1, Some("alice"), None),
            copy(2, Some("alice"), Some(5)),
        ];
        assert_eq!(suggested_index(&group), 1);
        let group = [
            copy(3, None, None),
            copy(4, None, Some(5)),
            copy(5, None, Some(5)),
        ];
        assert_eq!(suggested_index(&group), 1);
        // Then the oldest upload
        let group = [copy(7, Some("alice"), None), copy(6, Some("alice"), None)];
        assert_eq!(suggested_index(&group), 1);
        let group = [copy(6, Some("alice"), None), copy(7, Some("alice"), None)];
        assert_eq!(suggested_index(&group), 0);
    }

    #[test]
    fn test_bulk_kind() {
        let alice = Some("alice");
        // One Personal and one Family copy
        assert_eq!(
            bulk_kind(&[copy(1, alice, Some(5)), copy(2, None, None)]),
            Some(BulkKind::KeepFamily)
        );
        // Same owner and folder, public or not
        assert_eq!(
            bulk_kind(&[
                copy(1, alice, None),
                copy(2, alice, None),
                copy(3, alice, None)
            ]),
            Some(BulkKind::RemoveExtra)
        );
        assert_eq!(
            bulk_kind(&[copy(1, None, Some(5)), copy(2, None, Some(5))]),
            Some(BulkKind::RemoveExtra)
        );
        // Different folders, or three copies with both owners
        assert_eq!(
            bulk_kind(&[copy(1, alice, Some(5)), copy(2, alice, None)]),
            None
        );
        assert_eq!(
            bulk_kind(&[copy(1, None, Some(5)), copy(2, None, Some(6))]),
            None
        );
        assert_eq!(
            bulk_kind(&[
                copy(1, alice, None),
                copy(2, alice, None),
                copy(3, None, None)
            ]),
            None
        );
        assert_eq!(bulk_kind(&[]), None);
    }

    #[test]
    fn test_group_view_frees_all_but_the_suggested_copy() {
        let group = [copy(1, Some("alice"), None), copy(2, None, None)];
        let view = GroupView::new(&group);
        assert_eq!(view.freed_bytes, 100);
        assert_eq!(view.freed, "100 B");
        assert_eq!(view.bulk, "family");
        assert_eq!(
            view.copies.iter().map(|c| c.suggested).collect::<Vec<_>>(),
            [false, true]
        );
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
