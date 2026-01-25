use crate::http::AppStateRef;
use crate::http::error::HttpResult;
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::utils::AuthSession;
use crate::model::photo::Photo;
use crate::repo::PhotoCursor;
use crate::repo::{FavoritesRepo, PaginatedPhotos, PhotosRepo, PhotosTransactionRepo};
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use time::{Month, OffsetDateTime};

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PhotoCategory {
    #[default]
    All,
    Personal,
    Family,
}

impl fmt::Display for PhotoCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PhotoCategory::All => write!(f, "all"),
            PhotoCategory::Personal => write!(f, "personal"),
            PhotoCategory::Family => write!(f, "family"),
        }
    }
}

impl PhotoCategory {
    /// Convert category to (personal_only, family_only) filter flags
    pub fn to_filters(self) -> (bool, bool) {
        match self {
            PhotoCategory::All => (false, false),
            PhotoCategory::Personal => (true, false),
            PhotoCategory::Family => (false, true),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct GalleryQuery {
    #[serde(default)]
    pub category: PhotoCategory,
}

pub struct GroupedFolders {
    pub personal: Vec<String>,
    pub family: Vec<String>,
}

#[derive(Template)]
#[template(path = "gallery/gallery_page.html")]
struct GalleryPageTemplate {
    groups: Vec<MonthGroup>,
    personal_folders: Vec<String>,
    family_folders: Vec<String>,
    current_category: PhotoCategory,
    next_cursor: Option<String>,
    has_more: bool,
    last_month: Option<String>,
}

#[derive(Template)]
#[template(path = "gallery/photo_grid.html")]
struct PhotoGridTemplate {
    groups: Vec<MonthGroup>,
    current_category: PhotoCategory,
    next_cursor: Option<String>,
    has_more: bool,
    last_month: Option<String>,
}

#[derive(Template)]
#[template(path = "gallery/photo_batch.html")]
pub struct PhotoBatchTemplate {
    pub groups: Vec<MonthGroup>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub last_month: Option<String>,
    pub load_more_url: String,
    pub category: Option<PhotoCategory>,
}

#[derive(Template)]
#[template(path = "gallery/folder_page.html")]
struct FolderPageTemplate {
    groups: Vec<MonthGroup>,
    personal_folders: Vec<String>,
    family_folders: Vec<String>,
    current_folder: Option<String>,
    next_cursor: Option<String>,
    has_more: bool,
    last_month: Option<String>,
}

#[derive(Template)]
#[template(path = "gallery/photo_modal.html")]
struct PhotoModalTemplate {
    photo: Photo,
    is_favorite: bool,
}

pub struct PhotoView {
    pub id: i64,
    pub name: String,
    pub is_favorite: bool,
    pub thumb_hash: Option<String>,
    pub created_at: OffsetDateTime,
}

impl PhotoView {
    pub fn from_photo(photo: Photo, favorites: &HashSet<i64>) -> Self {
        let thumb_hash = photo.thumb_hash.as_ref().map(|h| STANDARD.encode(h));

        Self {
            id: photo.id,
            name: photo.name,
            is_favorite: favorites.contains(&photo.id),
            thumb_hash,
            created_at: photo.created_at,
        }
    }
}

pub const PAGE_SIZE: i64 = 100;

/// A group of photos from the same month/year
pub struct MonthGroup {
    pub label: String,
    pub photos: Vec<PhotoView>,
    pub show_header: bool,
}

/// Query parameters for paginated endpoints
#[derive(Debug, Default, Deserialize)]
pub struct PaginatedQuery {
    pub cursor: Option<String>,
    pub last_month: Option<String>,
    #[serde(default)]
    pub category: PhotoCategory,
}

/// Encode a cursor to a URL-safe base64 string
pub fn encode_cursor(cursor: &PhotoCursor) -> String {
    let json = serde_json::to_string(cursor).unwrap_or_default();
    URL_SAFE_NO_PAD.encode(json.as_bytes())
}

/// Decode a cursor from a URL-safe base64 string
pub fn decode_cursor(encoded: &str) -> Option<PhotoCursor> {
    let bytes = URL_SAFE_NO_PAD.decode(encoded).ok()?;
    let json = String::from_utf8(bytes).ok()?;
    serde_json::from_str(&json).ok()
}

fn format_month_label(year: i32, month: Month) -> String {
    format!("{month} {year}")
}

/// Parse "YYYY-MM" format to (year, month) tuple
pub fn parse_month_key(key: &str) -> Option<(i32, Month)> {
    let parts: Vec<&str> = key.split('-').collect();
    if parts.len() != 2 {
        return None;
    }
    let year: i32 = parts[0].parse().ok()?;
    let month = Month::try_from(parts[1].parse::<u8>().ok()?).ok()?;
    Some((year, month))
}

/// Format (year, month) to "YYYY-MM" string
fn format_month_key(year: i32, month: Month) -> String {
    format!("{:04}-{:02}", year, month as u8)
}

/// Group photos by month, optionally skipping the header for the first month
pub fn group_photos_by_month(
    photos: Vec<PhotoView>,
    skip_first_month: Option<(i32, Month)>,
) -> Vec<MonthGroup> {
    if photos.is_empty() {
        return Vec::new();
    }

    let mut groups: Vec<MonthGroup> = Vec::new();
    let mut current_group: Option<((i32, Month), Vec<PhotoView>)> = None;

    for photo in photos {
        let year = photo.created_at.year();
        let month = photo.created_at.month();
        let key = (year, month);

        match &mut current_group {
            Some((group_key, group_photos)) if *group_key == key => {
                group_photos.push(photo);
            }
            _ => {
                // Save the previous group
                if let Some((group_key, group_photos)) = current_group.take() {
                    let show_header = skip_first_month != Some(group_key) || !groups.is_empty();
                    groups.push(MonthGroup {
                        label: format_month_label(group_key.0, group_key.1),
                        photos: group_photos,
                        show_header,
                    });
                }
                // Start a new group
                current_group = Some((key, vec![photo]));
            }
        }
    }

    // Don't forget the last group
    if let Some((group_key, group_photos)) = current_group {
        let show_header = skip_first_month != Some(group_key) || !groups.is_empty();
        groups.push(MonthGroup {
            label: format_month_label(group_key.0, group_key.1),
            photos: group_photos,
            show_header,
        });
    }

    groups
}

/// Get the last month from a list of groups
pub fn get_last_month(groups: &[MonthGroup]) -> Option<String> {
    groups.last().and_then(|g| {
        g.photos
            .last()
            .map(|p| format_month_key(p.created_at.year(), p.created_at.month()))
    })
}

/// Processed photos ready for rendering in templates
pub struct ProcessedPhotos {
    pub groups: Vec<MonthGroup>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub last_month: Option<String>,
}

impl ProcessedPhotos {
    /// Convert paginated photos into a processed result ready for templates
    pub fn from_paginated(
        paginated: PaginatedPhotos,
        favorite_ids: &HashSet<i64>,
        skip_month: Option<(i32, Month)>,
    ) -> Self {
        let photo_views: Vec<PhotoView> = paginated
            .photos
            .into_iter()
            .map(|p| PhotoView::from_photo(p, favorite_ids))
            .collect();

        let groups = group_photos_by_month(photo_views, skip_month);
        let last_month = get_last_month(&groups);
        let next_cursor = paginated.next_cursor.as_ref().map(encode_cursor);

        Self {
            groups,
            next_cursor,
            has_more: paginated.has_more,
            last_month,
        }
    }
}

pub fn extract_grouped_folders(photos: &[Photo], user_id: &str) -> GroupedFolders {
    let mut personal_folders: HashSet<String> = HashSet::new();
    let mut family_folders: HashSet<String> = HashSet::new();

    for photo in photos {
        if let Some(folder) = &photo.folder {
            if folder.is_empty() {
                continue;
            }
            if photo.user_id.as_deref() == Some(user_id) {
                personal_folders.insert(folder.clone());
            } else if photo.user_id.is_none() {
                family_folders.insert(folder.clone());
            }
        }
    }

    let mut personal: Vec<String> = personal_folders.into_iter().collect();
    let mut family: Vec<String> = family_folders.into_iter().collect();
    personal.sort();
    family.sort();

    GroupedFolders { personal, family }
}

pub struct PhotosData {
    pub photos: Vec<Photo>,
    pub favorite_ids: HashSet<i64>,
}

pub async fn fetch_photos_and_favorites(
    state: &AppStateRef,
    user_id: &str,
) -> Result<PhotosData, sqlx::Error> {
    let mut tx = state.pool.begin().await?;
    let photos = tx.get_photos_by_user_and_public(user_id).await?.photos;
    let favorite_ids = tx.get_favorite_photos(user_id).await?.into_iter().collect();
    tx.commit().await?;
    Ok(PhotosData {
        photos,
        favorite_ids,
    })
}

pub async fn gallery_page(
    auth_session: AuthSession,
    State(state): State<AppStateRef>,
    Query(query): Query<GalleryQuery>,
) -> HttpResult<Response> {
    let user = auth_session.user.expect("User must be authenticated");
    let category = query.category;
    let (personal_only, family_only) = category.to_filters();

    let mut tx = state.pool.begin().await?;

    // Get all photos for folder listing (we need full list for sidebar)
    let all_photos = tx.get_photos_by_user_and_public(&user.id).await?.photos;
    let grouped_folders = extract_grouped_folders(&all_photos, &user.id);

    // Get paginated photos
    let paginated = tx
        .get_photos_paginated(&user.id, personal_only, family_only, None, PAGE_SIZE)
        .await?;

    let favorite_ids: HashSet<i64> = tx
        .get_favorite_photos(&user.id)
        .await?
        .into_iter()
        .collect();
    tx.commit().await?;

    let processed = ProcessedPhotos::from_paginated(paginated, &favorite_ids, None);

    GalleryPageTemplate {
        groups: processed.groups,
        personal_folders: grouped_folders.personal,
        family_folders: grouped_folders.family,
        current_category: category,
        next_cursor: processed.next_cursor,
        has_more: processed.has_more,
        last_month: processed.last_month,
    }
    .try_into_response()
}

pub async fn photo_grid(
    auth_session: AuthSession,
    State(state): State<AppStateRef>,
    Query(query): Query<GalleryQuery>,
) -> HttpResult<Response> {
    let user = auth_session.user.expect("User must be authenticated");
    let category = query.category;
    let (personal_only, family_only) = category.to_filters();

    let mut tx = state.pool.begin().await?;
    let paginated = tx
        .get_photos_paginated(&user.id, personal_only, family_only, None, PAGE_SIZE)
        .await?;
    let favorite_ids: HashSet<i64> = tx
        .get_favorite_photos(&user.id)
        .await?
        .into_iter()
        .collect();
    tx.commit().await?;

    let processed = ProcessedPhotos::from_paginated(paginated, &favorite_ids, None);

    PhotoGridTemplate {
        groups: processed.groups,
        current_category: category,
        next_cursor: processed.next_cursor,
        has_more: processed.has_more,
        last_month: processed.last_month,
    }
    .try_into_response()
}

pub async fn folder_page(
    auth_session: AuthSession,
    State(state): State<AppStateRef>,
    Path(folder_name): Path<String>,
) -> HttpResult<Response> {
    let user = auth_session.user.expect("User must be authenticated");

    let mut tx = state.pool.begin().await?;

    // Get all photos for folder listing (we need full list for sidebar)
    let all_photos = tx.get_photos_by_user_and_public(&user.id).await?.photos;
    let grouped_folders = extract_grouped_folders(&all_photos, &user.id);

    // Get paginated photos for this folder
    let paginated = tx
        .get_folder_photos_paginated(&user.id, &folder_name, None, PAGE_SIZE)
        .await?;

    let favorite_ids: HashSet<i64> = tx
        .get_favorite_photos(&user.id)
        .await?
        .into_iter()
        .collect();
    tx.commit().await?;

    let processed = ProcessedPhotos::from_paginated(paginated, &favorite_ids, None);

    FolderPageTemplate {
        groups: processed.groups,
        personal_folders: grouped_folders.personal,
        family_folders: grouped_folders.family,
        current_folder: Some(folder_name),
        next_cursor: processed.next_cursor,
        has_more: processed.has_more,
        last_month: processed.last_month,
    }
    .try_into_response()
}

pub async fn load_more_gallery(
    auth_session: AuthSession,
    State(state): State<AppStateRef>,
    Query(query): Query<PaginatedQuery>,
) -> HttpResult<Response> {
    let user = auth_session.user.expect("User must be authenticated");
    let category = query.category;
    let (personal_only, family_only) = category.to_filters();

    let cursor = query.cursor.as_ref().and_then(|c| decode_cursor(c));
    let skip_month = query.last_month.as_ref().and_then(|m| parse_month_key(m));

    let mut tx = state.pool.begin().await?;
    let paginated = tx
        .get_photos_paginated(
            &user.id,
            personal_only,
            family_only,
            cursor.as_ref(),
            PAGE_SIZE,
        )
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
        load_more_url: "/gallery/more".to_string(),
        category: Some(category),
    }
    .try_into_response()
}

pub async fn load_more_folder(
    auth_session: AuthSession,
    State(state): State<AppStateRef>,
    Path(folder_name): Path<String>,
    Query(query): Query<PaginatedQuery>,
) -> HttpResult<Response> {
    let user = auth_session.user.expect("User must be authenticated");

    let cursor = query.cursor.as_ref().and_then(|c| decode_cursor(c));
    let skip_month = query.last_month.as_ref().and_then(|m| parse_month_key(m));

    let mut tx = state.pool.begin().await?;
    let paginated = tx
        .get_folder_photos_paginated(&user.id, &folder_name, cursor.as_ref(), PAGE_SIZE)
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
        load_more_url: format!("/folder/{}/more", folder_name),
        category: None,
    }
    .try_into_response()
}

pub async fn photo_modal(
    auth_session: AuthSession,
    State(state): State<AppStateRef>,
    Path(photo_id): Path<i64>,
) -> HttpResult<Response> {
    let user = auth_session.user.expect("User must be authenticated");

    let mut tx = state.pool.begin().await?;
    let photo = tx
        .get_photo(photo_id, &user.id)
        .await?
        .ok_or(crate::http::error::HttpError::NotFound)?;

    let favorites: HashSet<i64> = tx
        .get_favorite_photos(&user.id)
        .await?
        .into_iter()
        .collect();
    tx.commit().await?;

    let is_favorite = favorites.contains(&photo.id);

    PhotoModalTemplate { photo, is_favorite }.try_into_response()
}
