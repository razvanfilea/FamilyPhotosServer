use crate::http::AppStateRef;
use crate::http::error::HttpResult;
use crate::http::pages::gallery::extract_grouped_folders;
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::utils::AuthSession;
use crate::repo::PhotosTransactionRepo;
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
    auth_session: AuthSession,
    State(state): State<AppStateRef>,
) -> HttpResult<Response> {
    let user = auth_session.user.expect("User must be authenticated");

    let mut tx = state.pool.begin().await?;
    let photos = tx.get_photos_by_user_and_public(&user.id).await?.photos;
    tx.commit().await?;

    let grouped_folders = extract_grouped_folders(&photos, &user.id);

    UploadPageTemplate {
        personal_folders: grouped_folders.personal,
        family_folders: grouped_folders.family,
    }
    .try_into_response()
}
