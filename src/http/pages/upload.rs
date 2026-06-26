use crate::http::AppStateRef;
use crate::http::auth::AuthenticatedUser;
use crate::http::error::HttpResult;
use crate::http::template_into_response::TemplateIntoResponse;
use crate::model::folder::AccessibleFolder;
use crate::repo::FoldersRepo;
use askama::Template;
use axum::extract::State;
use axum::response::Response;

#[derive(Template)]
#[template(path = "upload/upload_page.html")]
struct UploadPageTemplate {
    personal_folders: Vec<AccessibleFolder>,
    family_folders: Vec<AccessibleFolder>,
}

pub async fn upload_page(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppStateRef>,
) -> HttpResult<Response> {
    let folders = state.read_pool.get_accessible_folders(&user.id).await?;

    let (personal_folders, family_folders) = folders
        .into_iter()
        .filter(|f| f.can_upload)
        .partition(|f| f.owner_id.is_some());

    UploadPageTemplate {
        personal_folders,
        family_folders,
    }
    .try_into_response()
}
