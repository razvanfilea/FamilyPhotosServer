use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::utils::AuthSession;
use askama::Template;
use axum::response::{IntoResponse, Redirect};

pub async fn login_page(auth_session: AuthSession) -> impl IntoResponse {
    if auth_session.user.is_some() {
        return Redirect::to("/").into_response();
    }

    #[derive(Template)]
    #[template(path = "user/login_page.html")]
    struct LoginTemplate;

    LoginTemplate.into_response()
}
