use crate::http::error::HttpResult;
use crate::http::page_context::{Nav, PageContext};
use crate::http::template_into_response::TemplateIntoResponse;
use askama::Template;
use axum::response::Response;

#[derive(Template)]
#[template(path = "settings/settings_page.html")]
struct SettingsTemplate {
    ctx: PageContext,
    version: &'static str,
}

/// `GET /settings`: the account, the password and the version
pub async fn settings_page(ctx: PageContext) -> HttpResult<Response> {
    SettingsTemplate {
        ctx: ctx.nav(Nav::Utilities),
        version: env!("CARGO_PKG_VERSION"),
    }
    .try_into_response()
}
