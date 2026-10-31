//! Queries for [`PhotoListContext`]s.
//!
//! Every query is built from the same two helpers, [`push_from_where`] and [`order_by`], so that
//! month summaries, month pages, slices, positions and id lists always agree with each other.

use crate::http::context::{MAX_MEMORIES_YEARS_AGO, MIN_MEMORIES_YEARS_AGO, PhotoListContext};
use crate::model::photo::Photo;
use crate::model::photo_access::PhotoAccess;
use crate::model::photo_filter::PhotoFilter;
use sqlx::{FromRow, QueryBuilder, Sqlite, SqliteExecutor};

/// Photos larger than this are listed by [`PhotoListContext::LargeFiles`] (50 MiB)
pub const LARGE_FILE_MIN_SIZE: i64 = 50 * 1024 * 1024;

/// Maximum number of ids returned by [`ContextRepo::context_ids`]
pub const CONTEXT_IDS_LIMIT: i64 = 10_000;

/// The month expression used for grouping. `created_at` is always UTC ISO text, so its first 7
/// characters are the month. Must agree with the range [`push_month_filter`] uses.
const MONTH_EXPR: &str = "substr(p.created_at, 1, 7)";

const YEARS_AGO_EXPR: &str =
    "(cast(strftime('%Y', 'now') as integer) - cast(strftime('%Y', p.created_at) as integer))";

/// Same "not expired" expression as the one used by [`PhotoAccess`] queries in `photos_repo`.
const PERMISSION_NOT_EXPIRED: &str =
    "(fp.expires_at is null or datetime(fp.expires_at) > datetime('now'))";

/// How many days around today's day-of-year count as "on this day"
const MEMORIES_DAY_WINDOW: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct ContextPhoto {
    #[sqlx(flatten)]
    pub photo: Photo,
    /// Whether the user has [`PhotoAccess::Delete`] on the photo
    pub deletable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct MonthSummary {
    /// `YYYY-MM`
    pub month: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct MonthCover {
    /// `YYYY-MM`
    pub month: String,
    pub count: i64,
    /// The first photo of the month in context order
    pub cover_photo_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct MemorySummary {
    pub years_ago: u32,
    pub count: i64,
    pub cover_photo_id: i64,
    pub cover_thumb_hash: Option<Vec<u8>>,
}

/// Pushes a boolean expression that is true when `me` has `access` to the photo `p`.
pub fn push_access(qb: &mut QueryBuilder<Sqlite>, me: &str, access: PhotoAccess) {
    qb.push("(p.user_id is null or p.user_id = ")
        .push_bind(me.to_owned())
        .push(" or exists (select 1 from folder_permissions fp where fp.folder_id = p.folder_id and fp.grantee_id = ")
        .push_bind(me.to_owned())
        .push(" and ")
        .push(PERMISSION_NOT_EXPIRED);
    if matches!(access, PhotoAccess::Delete) {
        qb.push(" and fp.can_delete = true");
    }
    qb.push("))");
}

/// Builds `min(abs(d1 - d2), 366 - abs(d1 - d2)) <= window`, where `d` is the day of the year of
/// each SQL operand, so that e.g. Dec 30 and Jan 2 are 3 days apart.
fn day_of_year_window(today: &str, date: &str) -> String {
    let distance = format!(
        "abs(cast(strftime('%j', {today}) as integer) - cast(strftime('%j', {date}) as integer))"
    );
    format!("min({distance}, 366 - {distance}) <= {MEMORIES_DAY_WINDOW}")
}

fn push_filter_predicate(qb: &mut QueryBuilder<Sqlite>, me: &str, filter: PhotoFilter) {
    match filter {
        PhotoFilter::All => {
            push_access(qb, me, PhotoAccess::Read);
        }
        PhotoFilter::Personal => {
            qb.push("p.user_id = ").push_bind(me.to_owned());
        }
        PhotoFilter::Family => {
            qb.push("p.user_id is null");
        }
        PhotoFilter::Shared => {
            qb.push("p.user_id is not null and p.user_id <> ")
                .push_bind(me.to_owned())
                .push(" and exists (select 1 from folder_permissions fp where fp.folder_id = p.folder_id and fp.grantee_id = ")
                .push_bind(me.to_owned())
                .push(" and ")
                .push(PERMISSION_NOT_EXPIRED)
                .push(")");
        }
    }
    qb.push(" and p.trashed_on is null");
}

fn push_predicate(qb: &mut QueryBuilder<Sqlite>, me: &str, ctx: &PhotoListContext) {
    match *ctx {
        PhotoListContext::Timeline(filter) => push_filter_predicate(qb, me, filter),
        PhotoListContext::Folder(folder_id) => {
            qb.push("p.folder_id = ")
                .push_bind(folder_id)
                .push(" and p.trashed_on is null");
        }
        PhotoListContext::Favorites => {
            qb.push("p.id in (select photo_id from favorite_photos where user_id = ")
                .push_bind(me.to_owned())
                .push(") and ");
            push_access(qb, me, PhotoAccess::Read);
            qb.push(" and p.trashed_on is null");
        }
        PhotoListContext::Trash => {
            qb.push("p.trashed_on is not null and ");
            push_access(qb, me, PhotoAccess::Delete);
        }
        PhotoListContext::LargeFiles => {
            qb.push("p.file_size > ")
                .push_bind(LARGE_FILE_MIN_SIZE)
                .push(" and ");
            push_access(qb, me, PhotoAccess::Read);
            qb.push(" and p.trashed_on is null");
        }
        PhotoListContext::Memories { years_ago, filter } => {
            push_filter_predicate(qb, me, filter);
            qb.push(" and ")
                .push(day_of_year_window("'now'", "p.created_at"))
                .push(" and ")
                .push(YEARS_AGO_EXPR)
                .push(" = ")
                .push_bind(years_ago);
        }
    }
}

/// Pushes `from photos p where <context predicate>`.
fn push_from_where(qb: &mut QueryBuilder<Sqlite>, me: &str, ctx: &PhotoListContext) {
    qb.push(" from photos p where ");
    push_predicate(qb, me, ctx);
}

/// The ordering of a context. Always ends with the `id` tie-break.
fn order_by(ctx: &PhotoListContext) -> &'static str {
    match ctx {
        PhotoListContext::Trash => "p.trashed_on asc, p.id asc",
        PhotoListContext::LargeFiles => "p.file_size desc, p.id desc",
        PhotoListContext::Timeline(_)
        | PhotoListContext::Folder(_)
        | PhotoListContext::Favorites
        | PhotoListContext::Memories { .. } => "p.created_at desc, p.id desc",
    }
}

/// The primary ordering column and whether it is descending (the `id` tie-break follows the same
/// direction). Must agree with [`order_by`].
fn sort_key(ctx: &PhotoListContext) -> (&'static str, bool) {
    match ctx {
        PhotoListContext::Trash => ("trashed_on", false),
        PhotoListContext::LargeFiles => ("file_size", true),
        PhotoListContext::Timeline(_)
        | PhotoListContext::Folder(_)
        | PhotoListContext::Favorites
        | PhotoListContext::Memories { .. } => ("created_at", true),
    }
}

fn push_select_photos(qb: &mut QueryBuilder<Sqlite>, me: &str, ctx: &PhotoListContext) {
    qb.push("select p.*, ");
    push_access(qb, me, PhotoAccess::Delete);
    qb.push(" as deletable");
    push_from_where(qb, me, ctx);
}

/// The month after `month` (`YYYY-MM`, validated by the caller)
fn next_month(month: &str) -> String {
    let year: u32 = month[..4].parse().expect("validated month");
    match month[5..].parse::<u32>().expect("validated month") {
        12 => format!("{:04}-01", year + 1),
        number => format!("{year:04}-{:02}", number + 1),
    }
}

/// Restricts the query to one month with a range, which the `created_at` indexes can serve
fn push_month_filter(qb: &mut QueryBuilder<Sqlite>, month: Option<&str>) {
    if let Some(month) = month {
        qb.push(" and p.created_at >= ")
            .push_bind(month.to_owned())
            .push(" and p.created_at < ")
            .push_bind(next_month(month));
    }
}

fn month_page_query(
    me: &str,
    ctx: &PhotoListContext,
    month: Option<&str>,
    offset: i64,
    limit: i64,
) -> QueryBuilder<Sqlite> {
    let mut qb = QueryBuilder::new("");
    push_select_photos(&mut qb, me, ctx);
    push_month_filter(&mut qb, month);
    qb.push(" order by ")
        .push(order_by(ctx))
        .push(" limit ")
        .push_bind(limit)
        .push(" offset ")
        .push_bind(offset);
    qb
}

pub trait ContextRepo<'c>: SqliteExecutor<'c> {
    /// Month sections of the context, newest month first.
    async fn context_month_summaries(
        self,
        me: &str,
        ctx: &PhotoListContext,
    ) -> sqlx::Result<Vec<MonthSummary>> {
        let mut qb = QueryBuilder::new("select ");
        qb.push(MONTH_EXPR).push(" as month, count(*) as count");
        push_from_where(&mut qb, me, ctx);
        qb.push(" group by month order by month desc");

        qb.build_query_as().fetch_all(self).await
    }

    /// Index of the first photo of `month` in the context and the number of photos in it.
    /// Month-sectioned contexts only (newest first): the index counts the photos newer than the month.
    async fn context_month_bounds(
        self,
        me: &str,
        ctx: &PhotoListContext,
        month: &str,
    ) -> sqlx::Result<(i64, i64)> {
        let next = next_month(month);
        let mut qb = QueryBuilder::new("select coalesce(sum(p.created_at >= ");
        qb.push_bind(next.clone())
            .push("), 0), coalesce(sum(p.created_at >= ")
            .push_bind(month.to_owned())
            .push(" and p.created_at < ")
            .push_bind(next)
            .push("), 0)");
        push_from_where(&mut qb, me, ctx);

        qb.build_query_as().fetch_one(self).await
    }

    /// Month sections with the first photo of each, newest month first.
    async fn context_month_covers(
        self,
        me: &str,
        ctx: &PhotoListContext,
    ) -> sqlx::Result<Vec<MonthCover>> {
        let mut qb = QueryBuilder::new(
            "select month, count(*) as count, max(case when rn = 1 then id end) as cover_photo_id from (select p.id, ",
        );
        qb.push(MONTH_EXPR)
            .push(" as month, row_number() over (partition by ")
            .push(MONTH_EXPR)
            .push(" order by ")
            .push(order_by(ctx))
            .push(") as rn");
        push_from_where(&mut qb, me, ctx);
        qb.push(") group by month order by month desc");

        qb.build_query_as().fetch_all(self).await
    }

    async fn context_count(self, me: &str, ctx: &PhotoListContext) -> sqlx::Result<i64> {
        let mut qb = QueryBuilder::new("select count(*)");
        push_from_where(&mut qb, me, ctx);
        qb.build_query_scalar().fetch_one(self).await
    }

    /// A page of photos within one month (`YYYY-MM`, validated by the caller), or within the
    /// whole context when `month` is `None` (flat contexts).
    async fn context_month_page(
        self,
        me: &str,
        ctx: &PhotoListContext,
        month: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> sqlx::Result<Vec<ContextPhoto>> {
        month_page_query(me, ctx, month, offset, limit)
            .build_query_as()
            .fetch_all(self)
            .await
    }

    /// Ids of one month, or of the whole context when `month` is `None`, in context order.
    /// Capped at [`CONTEXT_IDS_LIMIT`].
    async fn context_ids(
        self,
        me: &str,
        ctx: &PhotoListContext,
        month: Option<&str>,
    ) -> sqlx::Result<Vec<i64>> {
        let mut qb = QueryBuilder::new("select p.id");
        push_from_where(&mut qb, me, ctx);
        push_month_filter(&mut qb, month);
        qb.push(" order by ")
            .push(order_by(ctx))
            .push(" limit ")
            .push_bind(CONTEXT_IDS_LIMIT);

        qb.build_query_scalar().fetch_all(self).await
    }

    /// 0-based index of the photo in context order, `None` if it isn't part of the context.
    async fn context_position(
        self,
        me: &str,
        ctx: &PhotoListContext,
        photo_id: i64,
    ) -> sqlx::Result<Option<i64>> {
        let (column, descending) = sort_key(ctx);
        let cmp = if descending { ">" } else { "<" };

        // The photo's own values are read with sub-queries (instead of being decoded and bound
        // again) so that the comparison is exactly the one `order by` uses.
        let mut qb = QueryBuilder::new("select case when exists (select 1");
        push_from_where(&mut qb, me, ctx);
        qb.push(" and p.id = ")
            .push_bind(photo_id)
            .push(") then (select count(*)");
        push_from_where(&mut qb, me, ctx);
        qb.push(format!(
            " and (p.{column} {cmp} (select {column} from photos where id = "
        ))
        .push_bind(photo_id)
        .push(format!(
            ") or (p.{column} = (select {column} from photos where id = "
        ))
        .push_bind(photo_id)
        .push(format!(") and p.id {cmp} "))
        .push_bind(photo_id)
        .push("))) end");

        qb.build_query_scalar().fetch_one(self).await
    }

    /// `limit` photos starting at context index `start`.
    async fn context_slice(
        self,
        me: &str,
        ctx: &PhotoListContext,
        start: i64,
        limit: i64,
    ) -> sqlx::Result<Vec<ContextPhoto>> {
        month_page_query(me, ctx, None, start, limit)
            .build_query_as()
            .fetch_all(self)
            .await
    }

    /// "On this day" groups for the timeline filter, ordered by `years_ago` ascending.
    async fn memories_summary(
        self,
        me: &str,
        filter: PhotoFilter,
    ) -> sqlx::Result<Vec<MemorySummary>> {
        let mut qb = QueryBuilder::new(
            "select years_ago, count(*) as count, max(case when rn = 1 then id end) as cover_photo_id, max(case when rn = 1 then thumb_hash end) as cover_thumb_hash from (select p.id, p.thumb_hash, ",
        );
        qb.push(YEARS_AGO_EXPR)
            .push(" as years_ago, row_number() over (partition by ")
            .push(YEARS_AGO_EXPR)
            .push(" order by ")
            .push(order_by(&PhotoListContext::Timeline(filter)))
            .push(") as rn from photos p where ");
        push_filter_predicate(&mut qb, me, filter);
        qb.push(" and ")
            .push(day_of_year_window("'now'", "p.created_at"))
            .push(") where years_ago between ")
            .push_bind(MIN_MEMORIES_YEARS_AGO)
            .push(" and ")
            .push_bind(MAX_MEMORIES_YEARS_AGO)
            .push(" group by years_ago order by years_ago");

        qb.build_query_as().fetch_all(self).await
    }
}

impl<'c, E> ContextRepo<'c> for E where E: SqliteExecutor<'c> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::error::HttpError;
    use crate::model::photo_filter::PhotoFilter;
    use crate::repo::FavoritesRepo;
    use crate::repo::tests::{Dataset, insert_photo, insert_users, photo};
    use sqlx::SqlitePool;
    use std::collections::HashMap;
    use time::macros::datetime;
    use time::{Duration, OffsetDateTime};

    const ME: &str = "alice";

    async fn all_photos(pool: &SqlitePool, ctx: &PhotoListContext) -> Vec<ContextPhoto> {
        let count = pool.context_count(ME, ctx).await.unwrap();
        let photos = pool.context_slice(ME, ctx, 0, count + 10).await.unwrap();
        assert_eq!(photos.len() as i64, count);
        photos
    }

    /// Checks the invariants that must hold for every context
    async fn check_invariants(pool: &SqlitePool, ctx: &PhotoListContext) {
        let all = all_photos(pool, ctx).await;
        let count = all.len() as i64;

        // Month summaries add up and describe the delivered pages exactly
        let summaries = pool.context_month_summaries(ME, ctx).await.unwrap();
        assert_eq!(summaries.iter().map(|s| s.count).sum::<i64>(), count);
        for pair in summaries.windows(2) {
            assert!(pair[0].month > pair[1].month, "months must be newest first");
        }
        let covers = pool.context_month_covers(ME, ctx).await.unwrap();
        assert_eq!(
            covers
                .iter()
                .map(|c| (&c.month, c.count))
                .collect::<Vec<_>>(),
            summaries
                .iter()
                .map(|s| (&s.month, s.count))
                .collect::<Vec<_>>()
        );

        if ctx.is_month_sectioned() {
            let mut concatenated = Vec::new();
            let mut start_index = 0;
            for (summary, cover) in summaries.iter().zip(&covers) {
                let bounds = pool
                    .context_month_bounds(ME, ctx, &summary.month)
                    .await
                    .unwrap();
                assert_eq!(bounds, (start_index, summary.count), "{}", summary.month);
                start_index += summary.count;

                // Small pages to exercise offsets
                let mut offset = 0;
                loop {
                    let page = pool
                        .context_month_page(ME, ctx, Some(&summary.month), offset, 1)
                        .await
                        .unwrap();
                    if page.is_empty() {
                        break;
                    }
                    offset += page.len() as i64;
                    concatenated.extend(page);
                }
                assert_eq!(offset, summary.count, "month {}", summary.month);

                let month_photos = &concatenated[concatenated.len() - summary.count as usize..];
                assert_eq!(month_photos[0].photo.id, cover.cover_photo_id);

                let ids = pool
                    .context_ids(ME, ctx, Some(&summary.month))
                    .await
                    .unwrap();
                let expected: Vec<i64> = month_photos.iter().map(|p| p.photo.id).collect();
                assert_eq!(ids, expected);
            }
            assert_eq!(concatenated, all);
        } else {
            let page = pool
                .context_month_page(ME, ctx, None, 0, count + 1)
                .await
                .unwrap();
            assert_eq!(page, all);
        }

        let ids = pool.context_ids(ME, ctx, None).await.unwrap();
        assert_eq!(ids, all.iter().map(|p| p.photo.id).collect::<Vec<_>>());

        for (index, photo) in all.iter().enumerate() {
            let position = pool
                .context_position(ME, ctx, photo.photo.id)
                .await
                .unwrap();
            assert_eq!(position, Some(index as i64));

            let slice = pool.context_slice(ME, ctx, index as i64, 2).await.unwrap();
            assert_eq!(slice[0], *photo);
        }
    }

    #[sqlx::test]
    async fn test_context_membership_and_order(pool: SqlitePool) {
        insert_users(&pool).await;
        let data = Dataset::create(&pool).await;
        // Trashed, expired, unshared and other users' favourites don't count
        for name in [
            "own_1",
            "public_a",
            "shared_1",
            "own_trashed",
            "expired_1",
            "unshared_1",
        ] {
            pool.insert_favorite(data.id(name), ME).await.unwrap();
        }
        pool.insert_favorite(data.id("own_2"), "bob").await.unwrap();

        use PhotoListContext::*;
        // (context, photos in order, the ones that aren't deletable)
        let cases: &[(PhotoListContext, &[&str], &[&str])] = &[
            (
                Timeline(PhotoFilter::All),
                // Equal timestamps: id desc
                &[
                    "shared_1",
                    "own_1",
                    "public_b",
                    "public_a",
                    "own_2",
                    "shared_delete_1",
                    "public_big",
                    "own_big",
                    "own_old",
                ],
                &["shared_1"],
            ),
            (
                Timeline(PhotoFilter::Personal),
                &["own_1", "own_2", "own_big", "own_old"],
                &[],
            ),
            (
                Timeline(PhotoFilter::Family),
                &["public_b", "public_a", "public_big"],
                &[],
            ),
            (
                Timeline(PhotoFilter::Shared),
                &["shared_1", "shared_delete_1"],
                &["shared_1"],
            ),
            (Folder(data.own_folder), &["own_2"], &[]),
            (Folder(data.shared_folder), &["shared_1"], &["shared_1"]),
            (Folder(data.shared_delete_folder), &["shared_delete_1"], &[]),
            (Favorites, &["shared_1", "own_1", "public_a"], &["shared_1"]),
            // Oldest trashed first, equal trashed_on: id asc. No `shared_trashed` (view only).
            (
                Trash,
                &["public_trashed", "own_trashed", "shared_delete_trashed"],
                &[],
            ),
            (
                LargeFiles,
                &["public_big", "own_big", "shared_delete_1"],
                &[],
            ),
        ];

        for (ctx, expected, not_deletable) in cases {
            let photos = all_photos(&pool, ctx).await;
            assert_eq!(
                data.names(photos.iter().map(|p| p.photo.id)),
                *expected,
                "{ctx}"
            );
            let actual_not_deletable = photos.iter().filter(|p| !p.deletable);
            assert_eq!(
                data.names(actual_not_deletable.map(|p| p.photo.id)),
                *not_deletable,
                "{ctx}"
            );
            check_invariants(&pool, ctx).await;
        }

        // Photos outside the context have no position
        let timeline = Timeline(PhotoFilter::All);
        for name in ["own_trashed", "unshared_1", "expired_1", "carol_root"] {
            let position = pool.context_position(ME, &timeline, data.id(name)).await;
            assert_eq!(position.unwrap(), None, "{name}");
        }
        let position = pool.context_position(ME, &timeline, 999_999).await;
        assert_eq!(position.unwrap(), None);
        let position = pool.context_position(ME, &Trash, data.id("own_1")).await;
        assert_eq!(position.unwrap(), None);

        // Bob sees his own photos as personal and nothing as shared
        let count = async |filter| pool.context_count("bob", &Timeline(filter)).await.unwrap();
        assert_eq!(count(PhotoFilter::Shared).await, 0);
        assert_eq!(count(PhotoFilter::Personal).await, 5);
    }

    #[test]
    fn test_next_month() {
        for (month, next) in [
            ("2024-06", "2024-07"),
            ("2024-09", "2024-10"),
            ("2024-11", "2024-12"),
            ("2024-12", "2025-01"),
            ("0999-12", "1000-01"),
        ] {
            assert_eq!(next_month(month), next, "{month}");
        }
    }

    /// Month pages are text ranges on `created_at`, so photos at the edges of a month, with a
    /// fraction of a second, and from an offset that was normalized to UTC must land in the right
    /// month and agree with the summaries.
    #[sqlx::test]
    async fn test_month_edges(pool: SqlitePool) {
        insert_users(&pool).await;
        let times = [
            datetime!(2024-05-31 23:59:59.999 UTC),
            datetime!(2024-06-01 00:00:00 UTC),
            datetime!(2024-06-30 23:59:59 UTC),
            datetime!(2024-07-01 00:00:00 UTC),
            datetime!(2024-12-31 23:59:59 UTC),
            datetime!(2025-01-01 00:00:00 UTC),
        ];
        for (index, created_at) in times.into_iter().enumerate() {
            let photo = Photo {
                created_at,
                ..photo(Some(ME), &format!("{index}.jpg"))
            };
            insert_photo(&pool, photo).await;
        }

        let ctx = PhotoListContext::Timeline(PhotoFilter::Personal);
        let summaries = pool.context_month_summaries(ME, &ctx).await.unwrap();
        let actual: Vec<_> = summaries
            .iter()
            .map(|s| (s.month.as_str(), s.count))
            .collect();
        assert_eq!(
            actual,
            [
                ("2025-01", 1),
                ("2024-12", 1),
                ("2024-07", 1),
                ("2024-06", 2),
                ("2024-05", 1)
            ]
        );
        check_invariants(&pool, &ctx).await;
    }

    #[sqlx::test]
    async fn test_folder_authorize(pool: SqlitePool) {
        insert_users(&pool).await;
        let data = Dataset::create(&pool).await;

        for (folder, allowed) in [
            (data.own_folder, true),
            (data.shared_folder, true),
            (data.shared_delete_folder, true),
            (data.expired_folder, false),
            (data.unshared_folder, false),
            (999_999, false),
        ] {
            let result = PhotoListContext::Folder(folder).authorize(ME, &pool).await;
            if allowed {
                assert!(result.is_ok(), "{folder}");
            } else {
                assert!(matches!(result, Err(HttpError::NotFound)), "{folder}");
            }
        }
    }

    #[sqlx::test]
    async fn test_memories(pool: SqlitePool) {
        insert_users(&pool).await;

        let now = OffsetDateTime::now_utc();
        // Feb 29 doesn't exist in most years
        let base = if now.month() == time::Month::February && now.day() == 29 {
            now - Duration::days(1)
        } else {
            now
        };
        let years_ago = |years: i32, days: i64| {
            base.replace_year(base.year() - years).unwrap() + Duration::days(days)
        };

        let rows = [
            ("1y", Some(ME), years_ago(1, 0)),
            ("1y_plus_2d", Some(ME), years_ago(1, 2)),
            ("1y_minus_2d", None, years_ago(1, -2)),
            ("1y_plus_10d", Some(ME), years_ago(1, 10)),
            ("2y", None, years_ago(2, 1)),
            ("11y", Some(ME), years_ago(11, 0)),
            ("bob_1y", Some("bob"), years_ago(1, 0)),
            ("this_year", Some(ME), now),
        ];
        let mut ids = HashMap::new();
        for (name, user_id, created_at) in rows {
            let photo = insert_photo(
                &pool,
                Photo {
                    created_at,
                    ..photo(user_id, name)
                },
            )
            .await;
            ids.insert(name, photo.id);
        }

        let summary = pool.memories_summary(ME, PhotoFilter::All).await.unwrap();
        assert!(summary.windows(2).all(|w| w[0].years_ago < w[1].years_ago));
        assert!(summary.iter().all(|s| (1..=10).contains(&s.years_ago)));

        // Near new year's eve the shifted dates cross into neighbouring years (which is correct,
        // `years_ago` compares calendar years), so exact groups are only checked mid-year.
        let mid_year = (10..=355).contains(&now.ordinal());
        let groups = |summary: &[MemorySummary]| -> Vec<(u32, i64)> {
            summary.iter().map(|s| (s.years_ago, s.count)).collect()
        };
        if mid_year {
            assert_eq!(groups(&summary), vec![(1, 3), (2, 1)]);
            assert_eq!(summary[0].cover_photo_id, ids["1y_plus_2d"]);
        }

        for group in &summary {
            let ctx = PhotoListContext::Memories {
                years_ago: group.years_ago,
                filter: PhotoFilter::All,
            };
            let photos = all_photos(&pool, &ctx).await;
            assert_eq!(photos.len() as i64, group.count);
            assert_eq!(photos[0].photo.id, group.cover_photo_id);
            check_invariants(&pool, &ctx).await;
        }

        let shared = pool.memories_summary(ME, PhotoFilter::Shared).await;
        assert!(shared.unwrap().is_empty());
        if mid_year {
            let family = pool.memories_summary(ME, PhotoFilter::Family).await;
            assert_eq!(groups(&family.unwrap()), vec![(1, 1), (2, 1)]);

            // The context respects the filter too
            let ctx = PhotoListContext::Memories {
                years_ago: 1,
                filter: PhotoFilter::Family,
            };
            let photos = all_photos(&pool, &ctx).await;
            assert_eq!(photos.len(), 1);
            assert_eq!(photos[0].photo.id, ids["1y_minus_2d"]);
        }
    }

    #[sqlx::test]
    async fn test_day_of_year_window(pool: SqlitePool) {
        let sql = format!("select {}", day_of_year_window("?1", "?2"));
        for (today, date, within) in [
            ("2025-06-10", "2021-06-10", true),
            ("2025-06-10", "2021-06-13", true),
            ("2025-06-10", "2021-06-07", true),
            ("2025-06-10", "2021-06-14", false),
            // Wrap around the new year
            ("2025-01-02", "2020-12-30", true),
            ("2024-12-30", "2021-01-02", true),
            ("2025-01-05", "2020-12-30", false),
        ] {
            let actual: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(sql.as_str()))
                .bind(today)
                .bind(date)
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(actual, within, "{today} {date}");
        }
    }
}
