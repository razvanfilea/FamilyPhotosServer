use crate::http::auth::AuthenticatedUser;
use crate::http::error::HttpResult;
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::AppStateRef;
use crate::repo::PhotosRepo;
use askama::Template;
use axum::extract::State;
use axum::response::Response;

#[derive(Template)]
#[template(path = "upload/upload_page.html")]
struct UploadPageTemplate {
    personal_folders: Vec<String>,
    family_folders: Vec<String>,
}

pub async fn upload_page(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppStateRef>,
) -> HttpResult<Response> {
    // Use optimized queries that only fetch distinct folder names
    let personal_folders = state.pool.get_distinct_personal_folders(&user.id).await?;
    let family_folders = state.pool.get_distinct_family_folders().await?;

    UploadPageTemplate {
        personal_folders,
        family_folders,
    }
    .try_into_response()
}
