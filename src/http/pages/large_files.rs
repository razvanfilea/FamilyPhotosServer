use crate::http::AppStateRef;
use crate::http::context::PhotoListContext;
use crate::http::error::HttpResult;
use crate::http::page_context::{Nav, PageContext};
use crate::http::pages::grid::{GridView, build_grid_view};
use crate::http::template_into_response::TemplateIntoResponse;
use askama::Template;
use axum::extract::State;
use axum::response::Response;

#[derive(Template)]
#[template(path = "large_files/large_files_page.html")]
struct LargeFilesTemplate {
    ctx: PageContext,
    grid: GridView,
}

/// `GET /large-files`: the photos and videos over 50 MB, largest first
pub async fn large_files_page(
    State(state): State<AppStateRef>,
    ctx: PageContext,
) -> HttpResult<Response> {
    let context = PhotoListContext::LargeFiles;
    let grid = build_grid_view(state, &ctx.user_id, &context, &ctx.prefs).await?;

    LargeFilesTemplate {
        ctx: ctx.nav(Nav::Utilities).viewer_context(&context),
        grid,
    }
    .try_into_response()
}
