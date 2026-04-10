use crate::http::AppStateRef;
use crate::http::auth::AuthenticatedUser;
use crate::http::error::HttpResult;
use crate::http::pages::gallery::GalleryQuery;
use crate::http::template_into_response::TemplateIntoResponse;
use crate::model::photo_category::PhotoCategory;
use crate::repo::{FolderInfo, PhotosRepo};
use askama::Template;
use axum::Json;
use axum::extract::{Query, State};
use axum::response::Response;

#[derive(Template)]
#[template(path = "folders/folders_page.html")]
struct FoldersPageTemplate {
    folders: Vec<FolderInfo>,
    category: PhotoCategory,
}

pub async fn folders_page(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppStateRef>,
    Query(query): Query<GalleryQuery>,
) -> HttpResult<Response> {
    let category = query.category;

    // Get folders with counts for the selected category
    let folders = state
        .read_pool
        .get_folders_with_counts(&user.id, category)
        .await?;

    FoldersPageTemplate { folders, category }.try_into_response()
}

/// JSON endpoint for folder list (used by move dialog)
pub async fn folders_list_json(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppStateRef>,
) -> HttpResult<Json<Vec<FolderInfo>>> {
    // Get all folders (personal and family)
    let folders = state
        .read_pool
        .get_folders_with_counts(&user.id, PhotoCategory::All)
        .await?;

    Ok(Json(folders))
}
