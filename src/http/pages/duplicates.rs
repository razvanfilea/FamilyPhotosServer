use crate::http::error::HttpResult;
use crate::http::page_context::{Nav, PageContext};
use crate::http::pages::placeholder::placeholder_page;
use axum::response::Response;

pub async fn duplicates_page(ctx: PageContext) -> HttpResult<Response> {
    placeholder_page(ctx.nav(Nav::Utilities), "Duplicates", "content_copy")
}
