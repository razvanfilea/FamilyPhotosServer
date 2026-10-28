use crate::http::error::HttpResult;
use crate::http::page_context::PageContext;
use crate::http::template_into_response::TemplateIntoResponse;
use askama::Template;
use axum::response::Response;

/// Temporary page for destinations whose work package hasn't landed yet
#[derive(Template)]
#[template(path = "placeholder.html")]
struct PlaceholderTemplate {
    ctx: PageContext,
    title: &'static str,
    icon: &'static str,
}

pub fn placeholder_page(
    ctx: PageContext,
    title: &'static str,
    icon: &'static str,
) -> HttpResult<Response> {
    PlaceholderTemplate { ctx, title, icon }.try_into_response()
}
