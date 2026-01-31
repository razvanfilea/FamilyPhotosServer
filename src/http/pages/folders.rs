use crate::http::AppStateRef;
use crate::http::error::HttpResult;
use crate::http::pages::gallery::{GalleryQuery, PhotoCategory};
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::utils::AuthSession;
use crate::repo::{FolderInfo, PhotosTransactionRepo};
use askama::Template;
use axum::extract::{Query, State};
use axum::response::Response;

#[derive(Template)]
#[template(path = "folders/folders_page.html")]
struct FoldersPageTemplate {
    folders: Vec<FolderInfo>,
    category: PhotoCategory,
}

pub async fn folders_page(
    auth_session: AuthSession,
    State(state): State<AppStateRef>,
    Query(query): Query<GalleryQuery>,
) -> HttpResult<Response> {
    let user = auth_session.user.expect("User must be authenticated");
    let category = query.category;
    let (personal_only, family_only) = category.to_filters();

    let mut tx = state.pool.begin().await?;

    // Get folders with counts for the selected category
    let folders = tx
        .get_folders_with_counts(&user.id, personal_only, family_only)
        .await?;

    tx.commit().await?;

    FoldersPageTemplate { folders, category }.try_into_response()
}
