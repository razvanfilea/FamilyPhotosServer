use crate::http::AppStateRef;
use crate::http::auth::AuthenticatedUser;
use crate::http::error::{HttpError, HttpResult};
use crate::http::pages::gallery::{
    MonthGroup, PAGE_SIZE, PaginatedQuery, PhotoBatchTemplate, ProcessedPhotos, parse_month_key,
    parse_optional_cursor,
};
use crate::http::template_into_response::TemplateIntoResponse;
use crate::model::photo_category::PhotoCategory;
use crate::repo::{FavoritesRepo, PhotosRepo};
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::Method;
use axum::response::Response;
use serde::Deserialize;
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

#[derive(Template)]
#[template(path = "components/favorite_button.html")]
struct FavoriteButtonTemplate {
    photo_id: i64,
    is_favorite: bool,
}

#[derive(Template)]
#[template(path = "components/viewer_favorite_button.html")]
struct ViewerFavoriteButtonTemplate {
    photo_id: i64,
    is_favorite: bool,
}

#[derive(Debug, Deserialize)]
pub struct FavoriteQuery {
    source: Option<String>,
}

pub async fn toggle_favorite(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
    Query(query): Query<FavoriteQuery>,
    method: Method,
) -> HttpResult<Response> {
    let mut tx = state.pool.begin().await?;
    tx.get_photo(photo_id, &user.id)
        .await?
        .ok_or(HttpError::NotFound)?;

    let is_favorite = method == Method::POST;
    if is_favorite {
        tx.insert_favorite(photo_id, &user.id).await?;
    } else {
        tx.delete_favorite(photo_id, &user.id).await?;
    }
    tx.commit().await?;

    if query.source.as_deref() == Some("viewer") {
        ViewerFavoriteButtonTemplate {
            photo_id,
            is_favorite,
        }
        .try_into_response()
    } else {
        FavoriteButtonTemplate {
            photo_id,
            is_favorite,
        }
        .try_into_response()
    }
}
