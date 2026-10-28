use crate::http::AppStateRef;
use crate::http::error::HttpResult;
use crate::http::page_context::{Nav, PageContext};
use crate::http::template_into_response::TemplateIntoResponse;
use crate::model::folder::AccessibleFolder;
use crate::repo::FoldersRepo;
use askama::Template;
use axum::extract::State;
use axum::response::Response;

#[derive(Template)]
#[template(path = "upload/upload_page.html")]
struct UploadPageTemplate {
    ctx: PageContext,
    personal_folders: Vec<AccessibleFolder>,
    family_folders: Vec<AccessibleFolder>,
}

pub async fn upload_page(
    ctx: PageContext,
    State(state): State<AppStateRef>,
) -> HttpResult<Response> {
    let folders = state.read_pool.get_accessible_folders(&ctx.user_id).await?;

    let (personal_folders, family_folders) = folders
        .into_iter()
        .filter(|f| f.can_upload)
        .partition(|f| f.owner_id.is_some());

    UploadPageTemplate {
        ctx: ctx.nav(Nav::Upload),
        personal_folders,
        family_folders,
    }
    .try_into_response()
}
