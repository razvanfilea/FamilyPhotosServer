use crate::http::AppStateRef;
use crate::http::auth::AuthenticatedUser;
use crate::http::context::{ContextQuery, is_valid_month};
use crate::http::error::{HttpError, HttpResult};
use crate::repo::{ContextPhoto, ContextRepo, FavoritesRepo};
use crate::utils::media::is_video;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::serde::timestamp;

const DEFAULT_NEIGHBORS: i64 = 30;
const MAX_NEIGHBORS: i64 = 100;
/// Maximum `limit` when extending a window without a `photo_id`
const MAX_WINDOW: i64 = 2 * MAX_NEIGHBORS + 1;

pub fn router() -> Router<AppStateRef> {
    Router::new()
        .route("/neighbors", get(neighbors))
        .route("/ids", get(context_ids))
}

/// A photo as the viewer needs it
#[derive(Debug, Serialize)]
pub struct ViewerPhoto {
    pub id: i64,
    pub name: String,
    #[serde(with = "timestamp")]
    pub created_at: OffsetDateTime,
    pub is_video: bool,
    #[serde(with = "crate::utils::serde_helpers::base64_option")]
    pub thumb_hash: Option<Vec<u8>>,
    pub file_size: i64,
    pub user_id: Option<String>,
    pub folder_id: Option<i64>,
    pub is_favorite: bool,
    #[serde(with = "timestamp::option")]
    pub trashed_on: Option<OffsetDateTime>,
    /// Whether the user can trash/move the photo
    pub deletable: bool,
}

impl ViewerPhoto {
    fn new(context_photo: ContextPhoto, is_favorite: bool) -> Self {
        let photo = context_photo.photo;
        Self {
            id: photo.id,
            is_video: is_video(&photo.name),
            name: photo.name,
            created_at: photo.created_at,
            thumb_hash: photo.thumb_hash,
            file_size: photo.file_size,
            user_id: photo.user_id,
            folder_id: photo.folder_id,
            is_favorite,
            trashed_on: photo.trashed_on,
            deletable: context_photo.deletable,
        }
    }
}

#[derive(Debug, Serialize)]
struct NeighborsResponse {
    /// Context index of `photo_id`, `null` when requested by `start`
    index: Option<i64>,
    total: i64,
    /// Context index of `photos[0]`
    start: i64,
    photos: Vec<ViewerPhoto>,
}

#[derive(Deserialize)]
struct NeighborsQuery {
    photo_id: Option<i64>,
    before: Option<i64>,
    after: Option<i64>,
    start: Option<i64>,
    limit: Option<i64>,
}

/// `?ctx=&photo_id=&before=30&after=30` or `?ctx=&start=&limit=`
async fn neighbors(
    State(state): State<AppStateRef>,
    AuthenticatedUser(user): AuthenticatedUser,
    ContextQuery(ctx): ContextQuery,
    Query(query): Query<NeighborsQuery>,
) -> HttpResult<Json<NeighborsResponse>> {
    // One read transaction, so that the position, total and slice come from the same snapshot
    let mut tx = state.read_pool.begin().await?;

    let (index, start, limit) = match query.photo_id {
        Some(photo_id) => {
            let index = tx
                .context_position(&user.id, &ctx, photo_id)
                .await?
                .ok_or(HttpError::NotFound)?;
            let before = query
                .before
                .unwrap_or(DEFAULT_NEIGHBORS)
                .clamp(0, MAX_NEIGHBORS);
            let after = query
                .after
                .unwrap_or(DEFAULT_NEIGHBORS)
                .clamp(0, MAX_NEIGHBORS);
            let start = (index - before).max(0);
            (Some(index), start, index - start + 1 + after)
        }
        None => {
            let start = query.start.ok_or_else(|| {
                HttpError::BadRequest("Either photo_id or start is required".to_string())
            })?;
            let limit = query
                .limit
                .unwrap_or(DEFAULT_NEIGHBORS)
                .clamp(0, MAX_WINDOW);
            (None, start.max(0), limit)
        }
    };

    let total = tx.context_count(&user.id, &ctx).await?;
    let photos = tx.context_slice(&user.id, &ctx, start, limit).await?;

    let ids: Vec<i64> = photos.iter().map(|p| p.photo.id).collect();
    let favorites = tx.check_favorites_for_ids(&user.id, &ids).await?;
    let photos = photos
        .into_iter()
        .map(|p| {
            let is_favorite = favorites.contains(&p.photo.id);
            ViewerPhoto::new(p, is_favorite)
        })
        .collect();

    Ok(Json(NeighborsResponse {
        index,
        total,
        start,
        photos,
    }))
}

#[derive(Deserialize)]
struct IdsQuery {
    month: Option<String>,
}

/// `?ctx=&month=YYYY-MM` → ids of one month; without `month` → ids of the whole context
/// (capped at [`crate::repo::CONTEXT_IDS_LIMIT`]).
async fn context_ids(
    State(state): State<AppStateRef>,
    AuthenticatedUser(user): AuthenticatedUser,
    ContextQuery(ctx): ContextQuery,
    Query(query): Query<IdsQuery>,
) -> HttpResult<Json<Vec<i64>>> {
    let month = query.month.as_deref().filter(|m| !m.is_empty());
    if let Some(month) = month
        && !is_valid_month(month)
    {
        return Err(HttpError::BadRequest("Invalid month".to_string()));
    }

    Ok(Json(
        state.read_pool.context_ids(&user.id, &ctx, month).await?,
    ))
}
