use crate::http::AppStateRef;
use crate::http::context::PhotoListContext;
use crate::http::error::HttpResult;
use crate::http::page_context::{Nav, PageContext};
use crate::http::pages::grid::{GridView, build_grid_view};
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::views::plural;
use askama::Template;
use axum::extract::State;
use axum::response::Response;

#[derive(Template)]
#[template(path = "favorites/favorites_page.html")]
struct FavoritesTemplate {
    ctx: PageContext,
    count_label: String,
    grid: GridView,
}

/// `GET /favorites`: the photos the user marked as favourite
pub async fn favorites_page(
    State(state): State<AppStateRef>,
    ctx: PageContext,
) -> HttpResult<Response> {
    let context = PhotoListContext::Favorites;
    let grid = build_grid_view(state, &ctx.user_id, &context, &ctx.prefs).await?;

    FavoritesTemplate {
        ctx: ctx.nav(Nav::Favorites).viewer_context(&context),
        count_label: plural(grid.total, "photo"),
        grid,
    }
    .try_into_response()
}
