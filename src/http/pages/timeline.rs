use crate::http::AppStateRef;
use crate::http::context::PhotoListContext;
use crate::http::error::HttpResult;
use crate::http::page_context::{Nav, PageContext};
use crate::http::pages::grid::{GridView, build_grid_view};
use crate::http::template_into_response::TemplateIntoResponse;
use crate::model::photo_filter::PhotoFilter;
use crate::repo::ContextRepo;
use askama::Template;
use axum::extract::{Query, State};
use axum::response::Response;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;

/// A card of the "On this day" row
struct MemoryView {
    cover_photo_id: i64,
    thumb_hash_b64: String,
    context_key: String,
    label: String,
}

#[derive(Template)]
#[template(path = "timeline/timeline_page.html")]
struct TimelineTemplate {
    ctx: PageContext,
    filter: PhotoFilter,
    memories: Vec<MemoryView>,
    grid: GridView,
}

#[derive(Deserialize)]
pub struct TimelineQuery {
    filter: Option<String>,
}

fn years_ago_label(years_ago: u32) -> String {
    if years_ago == 1 {
        "1 year ago".to_string()
    } else {
        format!("{years_ago} years ago")
    }
}

/// The "On this day" cards for `filter`, nearest year first
async fn memories(
    state: AppStateRef,
    user_id: &str,
    filter: PhotoFilter,
) -> HttpResult<Vec<MemoryView>> {
    let mut memories = Vec::new();

    for summary in state.read_pool.memories_summary(user_id, filter).await? {
        let context = PhotoListContext::Memories {
            years_ago: summary.years_ago,
            filter,
        };
        // The cover is the first photo of the group; its thumbhash is the card's placeholder
        let cover = state
            .read_pool
            .context_month_page(user_id, &context, None, 0, 1)
            .await?
            .into_iter()
            .next();

        if let Some(cover) = cover {
            memories.push(MemoryView {
                cover_photo_id: cover.photo.id,
                thumb_hash_b64: cover
                    .photo
                    .thumb_hash
                    .as_ref()
                    .map(|hash| STANDARD.encode(hash))
                    .unwrap_or_default(),
                context_key: context.key(),
                label: years_ago_label(summary.years_ago),
            });
        }
    }

    Ok(memories)
}

/// `GET /`: the timeline. `?filter=` overrides the saved filter for this render.
pub async fn timeline_page(
    State(state): State<AppStateRef>,
    ctx: PageContext,
    Query(query): Query<TimelineQuery>,
) -> HttpResult<Response> {
    let prefs = ctx.prefs;
    let filter = query
        .filter
        .and_then(|filter| filter.parse().ok())
        .unwrap_or(prefs.filter);

    let grid = build_grid_view(
        state,
        &ctx.user_id,
        &PhotoListContext::Timeline(filter),
        &prefs,
    )
    .await?;
    let memories = memories(state, &ctx.user_id, filter).await?;

    TimelineTemplate {
        ctx: ctx.nav(Nav::Photos),
        filter,
        memories,
        grid,
    }
    .try_into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_years_ago_label() {
        assert_eq!(years_ago_label(1), "1 year ago");
        assert_eq!(years_ago_label(2), "2 years ago");
        assert_eq!(years_ago_label(10), "10 years ago");
    }
}
