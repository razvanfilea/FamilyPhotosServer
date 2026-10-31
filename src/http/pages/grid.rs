//! The photo grid shared by the Timeline, Folder, Favorites, Trash and Large files pages.
//!
//! Pages call [`build_grid_view`] and include the `photo_grid` macro (`templates/grid/macros.html`);
//! the `/ui/grid/*` fragment routes below serve the rest of the photos while the user scrolls.

use crate::http::AppStateRef;
use crate::http::auth::AuthenticatedUser;
use crate::http::context::{ContextQuery, PhotoListContext, is_valid_month};
use crate::http::error::{HttpError, HttpResult};
use crate::http::prefs::Prefs;
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::views::{DateStyle, PhotoTileView, format_bytes, format_date, plural};
use crate::model::photo::Photo;
use crate::repo::{ContextPhoto, ContextRepo};
use askama::Template;
use axum::extract::{Query, State};
use axum::response::Response;
use serde::Deserialize;
use time::{Date, Month, OffsetDateTime, Time};

/// Tiles per fragment: the size of a month page and of the tiles rendered with the page itself
pub const PAGE_SIZE: i64 = 200;

/// Days after which trashed photos are deleted for good, see `tasks::trash`
const TRASH_RETENTION_DAYS: i64 = 30;

/// Trashed photos with at most this many days left get the red badge
const TRASH_WARNING_DAYS: i64 = 7;

/// A tile in the grid: the shared photo tile plus what the grid adds around it
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileView {
    pub photo: PhotoTileView,
    /// Position of the photo in the whole context
    pub index: i64,
    /// Text of the badge in the bottom left corner, empty for none
    pub badge: String,
    /// `"error"` for the red badge, otherwise empty
    pub badge_kind: &'static str,
}

/// One month of the context, or the whole context for the flat ones (trash, large files)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionView {
    /// `YYYY-MM`, `None` for flat contexts
    pub month: Option<String>,
    /// `"June"` in the current year, `"June 2022"` otherwise
    pub label: String,
    /// `"June 2024"`
    pub label_with_year: String,
    pub count: i64,
    /// Index of the first photo of the section in the whole context
    pub start_index: i64,
    /// The photos rendered with the page, a prefix of the section. Empty for sections that are
    /// loaded by the browser when they come into view.
    pub tiles: Vec<TileView>,
}

impl SectionView {
    /// Offset of the next fragment page, `None` when the section is completely rendered
    pub fn next_offset(&self) -> Option<i64> {
        let loaded = self.tiles.len() as i64;
        (loaded > 0 && loaded < self.count).then_some(loaded)
    }
}

/// Everything the `photo_grid` macro renders
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridView {
    /// The `contextKey` of the photo list, see [`PhotoListContext::key`]
    pub context_key: String,
    /// One section without a header (trash, large files)
    pub flat: bool,
    pub zoom: u8,
    pub sections: Vec<SectionView>,
    /// Number of photos in the context
    pub total: i64,
}

/// A red or neutral badge shown on a tile
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Badge {
    pub text: String,
    pub error: bool,
}

/// The badge of a tile in `ctx`: the days left in the trash, or the size of large files
fn tile_badge(ctx: &PhotoListContext, photo: &Photo, now: OffsetDateTime) -> Option<Badge> {
    match ctx {
        PhotoListContext::Trash => {
            let days_left = (TRASH_RETENTION_DAYS - (now - photo.trashed_on?).whole_days()).max(0);
            Some(Badge {
                text: plural(days_left, "day"),
                error: days_left <= TRASH_WARNING_DAYS,
            })
        }
        PhotoListContext::LargeFiles => Some(Badge {
            text: format_bytes(photo.file_size),
            error: false,
        }),
        _ => None,
    }
}

fn build_tiles(
    ctx: &PhotoListContext,
    photos: &[ContextPhoto],
    first_index: i64,
    now: OffsetDateTime,
) -> Vec<TileView> {
    photos
        .iter()
        .zip(first_index..)
        .map(|(context_photo, index)| {
            let badge = tile_badge(ctx, &context_photo.photo, now);
            TileView {
                photo: PhotoTileView::from_photo(&context_photo.photo, context_photo.deletable),
                index,
                badge_kind: if badge.as_ref().is_some_and(|badge| badge.error) {
                    "error"
                } else {
                    ""
                },
                badge: badge.map(|badge| badge.text).unwrap_or_default(),
            }
        })
        .collect()
}

/// The first day of a `YYYY-MM` month, for formatting the labels
fn month_start(month: &str) -> Option<OffsetDateTime> {
    let (year, month) = month.split_once('-')?;
    let month = Month::try_from(month.parse::<u8>().ok()?).ok()?;
    let date = Date::from_calendar_date(year.parse().ok()?, month, 1).ok()?;
    Some(date.with_time(Time::MIDNIGHT).assume_utc())
}

fn month_labels(month: &str) -> (String, String) {
    match month_start(month) {
        Some(date) => (
            format_date(date, DateStyle::MonthHeader),
            format_date(date, DateStyle::MonthWithYear),
        ),
        None => (month.to_string(), month.to_string()),
    }
}

/// Builds the grid of `ctx` for `user_id`: the sections with their counts, which the browser turns
/// into reserved heights, and the first [`PAGE_SIZE`] tiles. The caller has checked that the user
/// may see `ctx` (a folder page does it when it loads the folder).
pub async fn build_grid_view(
    state: AppStateRef,
    user_id: &str,
    ctx: &PhotoListContext,
    prefs: &Prefs,
) -> HttpResult<GridView> {
    // One transaction, so the counts and the photos come from the same snapshot
    let mut tx = state.read_pool.begin().await?;
    let flat = !ctx.is_month_sectioned();

    let summaries = if flat {
        let count = tx.context_count(user_id, ctx).await?;
        vec![(None, count)]
    } else {
        tx.context_month_summaries(user_id, ctx)
            .await?
            .into_iter()
            .map(|summary| (Some(summary.month), summary.count))
            .collect()
    };
    let total: i64 = summaries.iter().map(|(_, count)| count).sum();

    // The photos of the first sections are rendered inline. Sections follow the context order,
    // so one slice covers all of them.
    let inline = tx
        .context_slice(user_id, ctx, 0, PAGE_SIZE.min(total))
        .await?;
    let now = OffsetDateTime::now_utc();

    let mut sections = Vec::with_capacity(summaries.len());
    let mut start_index = 0;
    for (month, count) in summaries {
        if count == 0 {
            continue;
        }

        let (label, label_with_year) = month.as_deref().map(month_labels).unwrap_or_default();
        let first = (start_index as usize).min(inline.len());
        let last = ((start_index + count) as usize).min(inline.len());

        sections.push(SectionView {
            month,
            label,
            label_with_year,
            count,
            start_index,
            tiles: build_tiles(ctx, &inline[first..last], start_index, now),
        });
        start_index += count;
    }

    Ok(GridView {
        context_key: ctx.key(),
        flat,
        zoom: prefs.zoom,
        sections,
        total,
    })
}

#[derive(Template)]
#[template(path = "grid/month_page.html")]
struct MonthPageTemplate {
    tiles: Vec<TileView>,
    /// Offset of the next page, `None` if this was the last
    next_offset: Option<i64>,
}

#[derive(Deserialize)]
pub struct MonthPageQuery {
    month: Option<String>,
    #[serde(default)]
    offset: i64,
}

/// `GET /ui/grid/month?ctx=&month=&offset=`: up to [`PAGE_SIZE`] tiles of one month (or of the
/// whole context for flat ones) and a `.month__more` sentinel when more remain
pub async fn month_page(
    State(state): State<AppStateRef>,
    AuthenticatedUser(user): AuthenticatedUser,
    ContextQuery(ctx): ContextQuery,
    Query(query): Query<MonthPageQuery>,
) -> HttpResult<Response> {
    if query.offset < 0 {
        return Err(HttpError::BadRequest("Invalid offset".to_string()));
    }

    let month = if ctx.is_month_sectioned() {
        let month = query
            .month
            .filter(|month| is_valid_month(month))
            .ok_or_else(|| HttpError::BadRequest("Invalid month".to_string()))?;
        Some(month)
    } else {
        None
    };

    let mut tx = state.read_pool.begin().await?;

    // The section's first index and size are recomputed here, never taken from the client
    let (start_index, count) = match month.as_deref() {
        Some(month) => tx.context_month_bounds(&user.id, &ctx, month).await?,
        None => (0, tx.context_count(&user.id, &ctx).await?),
    };

    let photos = tx
        .context_month_page(&user.id, &ctx, month.as_deref(), query.offset, PAGE_SIZE)
        .await?;
    let loaded_until = query.offset + photos.len() as i64;

    MonthPageTemplate {
        tiles: build_tiles(
            &ctx,
            &photos,
            start_index + query.offset,
            OffsetDateTime::now_utc(),
        ),
        next_offset: (loaded_until < count && !photos.is_empty()).then_some(loaded_until),
    }
    .try_into_response()
}

struct MonthPickerRow {
    month: String,
    label_with_year: String,
    count_label: String,
    cover_photo_id: i64,
}

#[derive(Template)]
#[template(path = "grid/month_picker.html")]
struct MonthPickerTemplate {
    rows: Vec<MonthPickerRow>,
}

/// `GET /ui/grid/month-picker?ctx=`: the "Select a Month" dialog content
pub async fn month_picker(
    State(state): State<AppStateRef>,
    AuthenticatedUser(user): AuthenticatedUser,
    ContextQuery(ctx): ContextQuery,
) -> HttpResult<Response> {
    if !ctx.is_month_sectioned() {
        return Err(HttpError::BadRequest("This list has no months".to_string()));
    }

    let rows = state
        .read_pool
        .context_month_covers(&user.id, &ctx)
        .await?
        .into_iter()
        .map(|summary| MonthPickerRow {
            label_with_year: month_labels(&summary.month).1,
            month: summary.month,
            count_label: plural(summary.count, "photo"),
            cover_photo_id: summary.cover_photo_id,
        })
        .collect();

    MonthPickerTemplate { rows }.try_into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::api::tests::TestServer;
    use crate::model::photo_filter::PhotoFilter;
    use crate::repo::PhotosTransactionRepo;
    use crate::repo::tests::{create_test_folder, photo};
    use axum::http::{HeaderName, StatusCode};
    use sqlx::SqlitePool;
    use time::Duration;
    use time::macros::datetime;

    /// Inserts photos for alice at the given times, returns their ids in insertion order
    async fn insert_photos(
        server: &TestServer,
        times: &[OffsetDateTime],
        folder_id: Option<i64>,
    ) -> Vec<i64> {
        let mut tx = server.pool().begin().await.unwrap();
        let mut ids = Vec::new();
        for (i, time) in times.iter().enumerate() {
            let photo = Photo {
                folder_id,
                created_at: *time,
                ..photo(Some("alice"), &format!("photo-{i}.jpg"))
            };
            ids.push(tx.insert_photo(&photo).await.unwrap().id);
        }
        tx.commit().await.unwrap();
        ids
    }

    fn timeline() -> PhotoListContext {
        PhotoListContext::Timeline(PhotoFilter::All)
    }

    /// 3 photos in June 2024 (two with the same timestamp), 1 in May 2024, 2 in December 2022
    fn sample_times() -> Vec<OffsetDateTime> {
        vec![
            datetime!(2024-06-08 14:03:29 UTC),
            datetime!(2024-06-08 14:03:29 UTC),
            datetime!(2024-06-01 09:00:00 UTC),
            datetime!(2024-05-31 23:59:59 UTC),
            datetime!(2022-12-25 12:00:00 UTC),
            datetime!(2022-12-24 12:00:00 UTC),
        ]
    }

    #[sqlx::test]
    async fn test_build_grid_view_indices_and_inline_tiles(pool: SqlitePool) {
        let server = TestServer::new(pool).await;
        let ids = insert_photos(&server, &sample_times(), None).await;

        let grid = build_grid_view(server.state, "alice", &timeline(), &Prefs::default())
            .await
            .unwrap();

        assert_eq!(grid.context_key, "timeline:all");
        assert!(!grid.flat);
        assert_eq!(grid.zoom, 1);
        assert_eq!(grid.total, 6);

        let months: Vec<_> = grid
            .sections
            .iter()
            .map(|s| (s.month.as_deref().unwrap(), s.count, s.start_index))
            .collect();
        assert_eq!(
            months,
            [("2024-06", 3, 0), ("2024-05", 1, 3), ("2022-12", 2, 4)]
        );

        // Labels: other years than the current one get the year
        assert_eq!(grid.sections[2].label, "December 2022");
        assert_eq!(grid.sections[2].label_with_year, "December 2022");
        assert_eq!(grid.sections[0].label_with_year, "June 2024");

        // Every tile is inline (6 < PAGE_SIZE), with its global index, in context order
        let tiles: Vec<_> = grid
            .sections
            .iter()
            .flat_map(|s| s.tiles.iter().map(|t| (t.index, t.photo.id)))
            .collect();
        assert_eq!(
            tiles.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
            (0..6).collect::<Vec<_>>()
        );
        // The equal-timestamp photos are ordered by id descending
        assert_eq!(tiles[0].1, ids[1]);
        assert_eq!(tiles[1].1, ids[0]);
        assert_eq!(tiles[2].1, ids[2]);
        assert!(grid.sections.iter().all(|s| s.next_offset().is_none()));
    }

    #[sqlx::test]
    async fn test_build_grid_view_renders_only_the_first_page(pool: SqlitePool) {
        let server = TestServer::new(pool).await;
        // 250 photos in one month, 3 in another
        let mut times: Vec<_> = (0..250)
            .map(|i| datetime!(2024-06-30 00:00:00 UTC) - Duration::minutes(i))
            .collect();
        times.extend([
            datetime!(2024-05-03 10:00:00 UTC),
            datetime!(2024-05-02 10:00:00 UTC),
            datetime!(2024-05-01 10:00:00 UTC),
        ]);
        insert_photos(&server, &times, None).await;

        let grid = build_grid_view(server.state, "alice", &timeline(), &Prefs::default())
            .await
            .unwrap();

        assert_eq!(grid.total, 253);
        assert_eq!(grid.sections[0].tiles.len(), 200);
        assert_eq!(grid.sections[0].next_offset(), Some(200));
        // The second section is not rendered at all and is loaded when it comes into view
        assert_eq!(grid.sections[1].start_index, 250);
        assert!(grid.sections[1].tiles.is_empty());
        assert_eq!(grid.sections[1].next_offset(), None);
    }

    #[sqlx::test]
    async fn test_build_grid_view_splits_the_inline_page_between_sections(pool: SqlitePool) {
        let server = TestServer::new(pool).await;
        let mut times: Vec<_> = (0..150)
            .map(|i| datetime!(2024-06-30 00:00:00 UTC) - Duration::minutes(i))
            .collect();
        times.extend((0..100).map(|i| datetime!(2024-05-30 00:00:00 UTC) - Duration::minutes(i)));
        insert_photos(&server, &times, None).await;

        let grid = build_grid_view(server.state, "alice", &timeline(), &Prefs::default())
            .await
            .unwrap();

        assert_eq!(grid.sections[0].tiles.len(), 150);
        assert_eq!(grid.sections[0].next_offset(), None);
        // The second section is partially rendered: 50 of 100
        assert_eq!(grid.sections[1].tiles.len(), 50);
        assert_eq!(grid.sections[1].tiles[0].index, 150);
        assert_eq!(grid.sections[1].next_offset(), Some(50));
    }

    #[sqlx::test]
    async fn test_build_grid_view_flat_and_empty(pool: SqlitePool) {
        let server = TestServer::new(pool).await;

        let grid = build_grid_view(server.state, "alice", &timeline(), &Prefs::default())
            .await
            .unwrap();
        assert_eq!(grid.total, 0);
        assert!(grid.sections.is_empty());

        let grid = build_grid_view(
            server.state,
            "alice",
            &PhotoListContext::LargeFiles,
            &Prefs::default(),
        )
        .await
        .unwrap();
        assert!(grid.flat);
        assert_eq!(grid.total, 0);
        assert!(grid.sections.is_empty());
    }

    #[sqlx::test]
    async fn test_build_grid_view_is_flat_for_trash(pool: SqlitePool) {
        let server = TestServer::new(pool).await;
        let photo = Photo {
            created_at: datetime!(2024-06-08 14:00:00 UTC),
            trashed_on: Some(OffsetDateTime::now_utc() - Duration::days(26)),
            ..photo(Some("alice"), "trashed.jpg")
        };
        let mut tx = server.pool().begin().await.unwrap();
        tx.insert_photo(&photo).await.unwrap();
        tx.commit().await.unwrap();

        let grid = build_grid_view(
            server.state,
            "alice",
            &PhotoListContext::Trash,
            &Prefs::default(),
        )
        .await
        .unwrap();

        assert!(grid.flat);
        assert_eq!(grid.sections.len(), 1);
        assert_eq!(grid.sections[0].month, None);
        assert_eq!(grid.sections[0].count, 1);
        let tile = &grid.sections[0].tiles[0];
        assert_eq!(tile.badge, "4 days");
        assert_eq!(tile.badge_kind, "error");
    }

    #[test]
    fn test_tile_badge() {
        let now = datetime!(2024-07-01 12:00:00 UTC);
        let photo = |trashed_days_ago: Option<i64>, file_size| Photo {
            created_at: now,
            file_size,
            trashed_on: trashed_days_ago.map(|days| now - Duration::days(days)),
            ..photo(None, "a.jpg")
        };
        let badge = |text: &str, error| {
            Some(Badge {
                text: text.into(),
                error,
            })
        };

        for (context, trashed_days_ago, file_size, expected) in [
            (PhotoListContext::Trash, Some(0), 1, badge("30 days", false)),
            (PhotoListContext::Trash, Some(22), 1, badge("8 days", false)),
            (PhotoListContext::Trash, Some(23), 1, badge("7 days", true)),
            (PhotoListContext::Trash, Some(29), 1, badge("1 day", true)),
            (PhotoListContext::Trash, Some(45), 1, badge("0 days", true)),
            (PhotoListContext::Trash, None, 1, None),
            (
                PhotoListContext::LargeFiles,
                None,
                125_829_100,
                badge("120 MB", false),
            ),
            (timeline(), None, 1, None),
        ] {
            let photo = photo(trashed_days_ago, file_size);
            assert_eq!(
                tile_badge(&context, &photo, now),
                expected,
                "{context:?} {trashed_days_ago:?} {file_size}"
            );
        }
    }

    #[test]
    fn test_month_labels() {
        let this_year = format!("{}-03", OffsetDateTime::now_utc().year());
        let (label, with_year) = month_labels("2022-03");
        assert_eq!(
            (label.as_str(), with_year.as_str()),
            ("March 2022", "March 2022")
        );
        assert_eq!(month_labels(&this_year).0, "March");
        assert_eq!(
            month_labels("garbage"),
            ("garbage".into(), "garbage".into())
        );
        assert_eq!(month_labels("2024-13").0, "2024-13");
    }

    #[sqlx::test]
    async fn test_month_fragment_pages_and_indices(pool: SqlitePool) {
        let server = TestServer::new(pool).await;
        let times: Vec<_> = (0..250)
            .map(|i| datetime!(2024-06-30 00:00:00 UTC) - Duration::minutes(i))
            .chain([datetime!(2024-05-03 10:00:00 UTC)])
            .collect();
        insert_photos(&server, &times, None).await;
        let alice = server.login("alice").await;
        let month = async |query: &str| {
            let response = alice
                .get(&format!("/ui/grid/month?ctx=timeline:all&{query}"))
                .await;
            let response = response.assert_status(StatusCode::OK);
            (response.count_testid("photo-tile"), response.text())
        };

        let (tiles, html) = month("month=2024-06&offset=0").await;
        assert_eq!(tiles, 200);
        assert!(html.contains(r#"data-index="0""#) && html.contains(r#"data-index="199""#));
        assert!(html.contains(r#"<div class="month__more" data-next-offset="200"></div>"#));

        let (tiles, html) = month("month=2024-06&offset=200").await;
        assert_eq!(tiles, 50);
        assert!(html.contains(r#"data-index="200""#) && html.contains(r#"data-index="249""#));
        assert!(!html.contains("month__more"));

        // The index of an older month starts after the newer ones, whatever the client says
        let (tiles, html) = month("month=2024-05&offset=0&start_index=7").await;
        assert_eq!(tiles, 1);
        assert!(html.contains(r#"data-index="250""#));

        // A month without photos gives an empty fragment
        assert_eq!(month("month=1999-01").await.0, 0);
    }

    #[sqlx::test]
    async fn test_month_fragment_rejects_bad_input(pool: SqlitePool) {
        let server = TestServer::new(pool).await;
        let alice = server.login("alice").await;

        for uri in [
            "/ui/grid/month?ctx=timeline:all&month=2024-6",
            "/ui/grid/month?ctx=timeline:all&month=2024-06-01",
            "/ui/grid/month?ctx=timeline:all",
            "/ui/grid/month?ctx=timeline:all&month=2024-06&offset=-1",
            "/ui/grid/month?ctx=nonsense&month=2024-06",
            "/ui/grid/month",
            "/ui/grid/month-picker?ctx=trash",
        ] {
            assert_eq!(
                alice.get(uri).await.status,
                StatusCode::BAD_REQUEST,
                "{uri}"
            );
        }
    }

    #[sqlx::test]
    async fn test_fragments_check_access(pool: SqlitePool) {
        let server = TestServer::new(pool).await;
        let folder_id = create_test_folder(server.pool(), Some("alice"), "Private")
            .await
            .id;
        insert_photos(
            &server,
            &[datetime!(2024-06-08 14:00:00 UTC)],
            Some(folder_id),
        )
        .await;
        let alice = server.login("alice").await;
        let bob = server.login("bob").await;

        let uri = format!("/ui/grid/month?ctx=folder:{folder_id}&month=2024-06");
        let response = alice.get(&uri).await.assert_status(StatusCode::OK);
        assert_eq!(response.count_testid("photo-tile"), 1);

        // Bob can't see alice's private folder
        bob.get(&uri).await.assert_status(StatusCode::NOT_FOUND);
        let picker = format!("/ui/grid/month-picker?ctx=folder:{folder_id}");
        bob.get(&picker).await.assert_status(StatusCode::NOT_FOUND);
    }

    #[sqlx::test]
    async fn test_fragments_redirect_to_login_when_logged_out(pool: SqlitePool) {
        let server = TestServer::new(pool).await;
        let current_url = [(
            HeaderName::from_static("hx-current-url"),
            "http://localhost:8080/?filter=family",
        )];

        for uri in [
            "/ui/grid/month?ctx=timeline:all&month=2024-06",
            "/ui/grid/month-picker?ctx=timeline:all",
        ] {
            let response = server.anonymous().get_with(uri, &current_url).await;
            let response = response.assert_status(StatusCode::UNAUTHORIZED);
            assert_eq!(
                response.header(HeaderName::from_static("hx-redirect")),
                "/login?next=%2F%3Ffilter%3Dfamily"
            );
        }
    }

    #[sqlx::test]
    async fn test_month_picker_lists_every_month(pool: SqlitePool) {
        let server = TestServer::new(pool).await;
        let ids = insert_photos(&server, &sample_times(), None).await;
        let alice = server.login("alice").await;

        let response = alice.get("/ui/grid/month-picker?ctx=timeline:all").await;
        let response = response.assert_status(StatusCode::OK);
        assert_eq!(response.count_testid("month-picker-row"), 3);
        let html = response.text();
        for expected in [
            "Select a Month",
            r#"data-month="2024-06""#,
            "June 2024",
            "3 photos",
            "1 photo<",
            &format!("/api/preview/{}", ids[1]),
        ] {
            assert!(html.contains(expected), "{expected}");
        }
        assert!(
            html.find("2024-06").unwrap() < html.find("2022-12").unwrap(),
            "newest first"
        );
    }
}
