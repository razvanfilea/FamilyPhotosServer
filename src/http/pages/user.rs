use crate::http::fragments::safe_redirect_path;
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::utils::AuthSession;
use askama::Template;
use axum::extract::Query;
use axum::response::{IntoResponse, Redirect};
use serde::Deserialize;

#[derive(Template)]
#[template(path = "user/login_page.html")]
struct LoginTemplate {
    /// Where to go after logging in, already validated as a safe relative path
    next: String,
}

#[derive(Deserialize)]
pub struct LoginQuery {
    next: Option<String>,
}

pub async fn login_page(
    auth_session: AuthSession,
    Query(query): Query<LoginQuery>,
) -> impl IntoResponse {
    let next = query
        .next
        .as_deref()
        .and_then(safe_redirect_path)
        .unwrap_or("/");

    if auth_session.user().await.is_some() {
        return Redirect::to(next).into_response();
    }

    LoginTemplate {
        next: next.to_string(),
    }
    .into_response()
}
