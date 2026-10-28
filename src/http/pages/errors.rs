use crate::http::page_context::PageContext;
use crate::http::prefs::Prefs;
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::utils::AuthSession;
use askama::Template;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Template)]
#[template(path = "errors/404.html")]
struct NotFoundTemplate {
    ctx: PageContext,
}

#[derive(Template)]
#[template(path = "errors/404_minimal.html")]
struct NotFoundMinimalTemplate;

/// The 404 page inside the shell, for handlers that find nothing to show
pub fn not_found_page(ctx: PageContext) -> Response {
    (
        StatusCode::NOT_FOUND,
        NotFoundTemplate { ctx }.into_response(),
    )
        .into_response()
}

/// Router fallback: the 404 page inside the shell when logged in, a minimal page otherwise
pub async fn not_found(auth: AuthSession, prefs: Prefs) -> Response {
    match auth.user().await {
        Some(user) => not_found_page(PageContext::new(&user, prefs)),
        None => (
            StatusCode::NOT_FOUND,
            NotFoundMinimalTemplate.into_response(),
        )
            .into_response(),
    }
}
