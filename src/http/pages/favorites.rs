use crate::http::AppStateRef;
use crate::http::auth::AuthenticatedUser;
use crate::http::error::HttpResult;
use crate::http::pages::gallery::{
    MonthGroup, PAGE_SIZE, PaginatedQuery, PhotoBatchTemplate, PhotoCategory, ProcessedPhotos,
    parse_month_key, parse_optional_cursor,
};
use crate::http::template_into_response::TemplateIntoResponse;
use crate::repo::PhotosRepo;
use askama::Template;
use axum::extract::{Query, State};
use axum::response::Response;
use std::collections::HashSet;

#[derive(Template)]
#[template(path = "favorites/favorites_page.html")]
struct FavoritesPageTemplate {
    groups: Vec<MonthGroup>,
    next_cursor: Option<String>,
    has_more: bool,
    last_month: Option<String>,
}

pub async fn favorites_page(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppStateRef>,
) -> HttpResult<Response> {
    let paginated = state
        .pool
        .get_favorite_photos_paginated(&user.id, None, PAGE_SIZE)
        .await?;

    let processed = ProcessedPhotos::from_paginated(paginated, &HashSet::default(), None);

    FavoritesPageTemplate {
        groups: processed.groups,
        next_cursor: processed.next_cursor,
        has_more: processed.has_more,
        last_month: processed.last_month,
    }
    .try_into_response()
}

pub async fn load_more_favorites(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppStateRef>,
    Query(query): Query<PaginatedQuery>,
) -> HttpResult<Response> {
    let cursor = parse_optional_cursor(query.cursor.as_deref())?;
    let skip_month = query.last_month.as_ref().and_then(|m| parse_month_key(m));

    let paginated = state
        .pool
        .get_favorite_photos_paginated(&user.id, cursor.as_ref(), PAGE_SIZE)
        .await?;

    let processed = ProcessedPhotos::from_paginated(paginated, &HashSet::default(), skip_month);

    PhotoBatchTemplate {
        groups: processed.groups,
        next_cursor: processed.next_cursor,
        has_more: processed.has_more,
        last_month: processed.last_month,
        load_more_url: "/favorites/more".to_string(),
        category: None,
    }
    .try_into_response()
}
