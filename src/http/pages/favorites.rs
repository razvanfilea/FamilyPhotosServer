use crate::http::error::HttpResult;
use crate::http::page_context::{Nav, PageContext};
use crate::http::pages::placeholder::placeholder_page;
use axum::response::Response;

pub async fn favorites_page(ctx: PageContext) -> HttpResult<Response> {
    placeholder_page(ctx.nav(Nav::Favorites), "Favorites", "star")
}
