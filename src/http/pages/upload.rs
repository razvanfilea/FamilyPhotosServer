use crate::http::AppStateRef;
use crate::http::auth::AuthenticatedUser;
use crate::http::error::HttpResult;
use crate::http::template_into_response::TemplateIntoResponse;
use crate::model::folder::Folder;
use crate::repo::FoldersRepo;
use askama::Template;
use axum::extract::State;
use axum::response::Response;

#[derive(Template)]
#[template(path = "upload/upload_page.html")]
struct UploadPageTemplate {
    personal_folders: Vec<Folder>,
    family_folders: Vec<Folder>,
}

pub async fn upload_page(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppStateRef>,
) -> HttpResult<Response> {
    let personal_folders = state
        .read_pool
        .get_folders_by_user_and_public(&user.id)
        .await?;

    let (personal_folders, family_folders): (Vec<Folder>, Vec<Folder>) = personal_folders
        .into_iter()
        .partition(|f| f.owner_id.is_some());

    UploadPageTemplate {
        personal_folders,
        family_folders,
    }
    .try_into_response()
}
