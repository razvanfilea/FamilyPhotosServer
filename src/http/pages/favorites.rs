use crate::http::AppStateRef;
use crate::http::error::HttpResult;
use crate::http::pages::gallery::{
    MonthGroup, PAGE_SIZE, PaginatedQuery, PhotoBatchTemplate, ProcessedPhotos, decode_cursor,
    extract_grouped_folders, parse_month_key,
};
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::utils::AuthSession;
use crate::repo::{FavoritesRepo, PhotosTransactionRepo};
use askama::Template;
use axum::extract::{Query, State};
use axum::response::Response;
use std::collections::HashSet;

#[derive(Template)]
#[template(path = "favorites/favorites_page.html")]
struct FavoritesPageTemplate {
    groups: Vec<MonthGroup>,
    personal_folders: Vec<String>,
    family_folders: Vec<String>,
    next_cursor: Option<String>,
    has_more: bool,
    last_month: Option<String>,
}

pub async fn favorites_page(
    auth_session: AuthSession,
    State(state): State<AppStateRef>,
) -> HttpResult<Response> {
    let user = auth_session.user.expect("User must be authenticated");

    let mut tx = state.pool.begin().await?;

    // Get all photos for folder listing
    let all_photos = tx.get_photos_by_user_and_public(&user.id).await?.photos;
    let grouped_folders = extract_grouped_folders(&all_photos, &user.id);

    // Get paginated favorite photos
    let paginated = tx
        .get_favorite_photos_paginated(&user.id, None, PAGE_SIZE)
        .await?;

    let favorite_ids: HashSet<i64> = tx
        .get_favorite_photos(&user.id)
        .await?
        .into_iter()
        .collect();
    tx.commit().await?;

    let processed = ProcessedPhotos::from_paginated(paginated, &favorite_ids, None);

    FavoritesPageTemplate {
        groups: processed.groups,
        personal_folders: grouped_folders.personal,
        family_folders: grouped_folders.family,
        next_cursor: processed.next_cursor,
        has_more: processed.has_more,
        last_month: processed.last_month,
    }
    .try_into_response()
}

pub async fn load_more_favorites(
    auth_session: AuthSession,
    State(state): State<AppStateRef>,
    Query(query): Query<PaginatedQuery>,
) -> HttpResult<Response> {
    let user = auth_session.user.expect("User must be authenticated");

    let cursor = query.cursor.as_ref().and_then(|c| decode_cursor(c));
    let skip_month = query.last_month.as_ref().and_then(|m| parse_month_key(m));

    let mut tx = state.pool.begin().await?;
    let paginated = tx
        .get_favorite_photos_paginated(&user.id, cursor.as_ref(), PAGE_SIZE)
        .await?;
    let favorite_ids: HashSet<i64> = tx
        .get_favorite_photos(&user.id)
        .await?
        .into_iter()
        .collect();
    tx.commit().await?;

    let processed = ProcessedPhotos::from_paginated(paginated, &favorite_ids, skip_month);

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
