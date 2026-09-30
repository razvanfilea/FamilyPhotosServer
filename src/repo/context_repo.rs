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

/// The month expression used for grouping. Must be identical in every month query.
const MONTH_EXPR: &str = "strftime('%Y-%m', p.created_at)";

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

#[allow(dead_code)] // TODO: used by the grid pages
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct MonthSummary {
    /// `YYYY-MM`
    pub month: String,
    pub count: i64,
    /// The first photo of the month in context order
    pub cover_photo_id: i64,
}

#[allow(dead_code)] // TODO: used by the memories row
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct MemorySummary {
    pub years_ago: u32,
    pub count: i64,
    pub cover_photo_id: i64,
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
            qb.push(
                "exists (select 1 from favorite_photos f where f.photo_id = p.id and f.user_id = ",
            )
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

fn push_month_filter(qb: &mut QueryBuilder<Sqlite>, month: Option<&str>) {
    if let Some(month) = month {
        qb.push(" and ")
            .push(MONTH_EXPR)
            .push(" = ")
            .push_bind(month.to_owned());
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
    #[allow(dead_code)] // TODO: used by the grid pages
    async fn context_month_summaries(
        self,
        me: &str,
        ctx: &PhotoListContext,
    ) -> sqlx::Result<Vec<MonthSummary>> {
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
    #[allow(dead_code)] // TODO: used by the grid pages
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
    #[allow(dead_code)] // TODO: used by the memories row
    async fn memories_summary(
        self,
        me: &str,
        filter: PhotoFilter,
    ) -> sqlx::Result<Vec<MemorySummary>> {
        let mut qb = QueryBuilder::new(
            "select years_ago, count(*) as count, max(case when rn = 1 then id end) as cover_photo_id from (select p.id, ",
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
    use crate::model::photo_filter::PhotoFilter;
    use crate::repo::tests::{
        create_test_folder, create_test_photo_with_time, create_test_user, insert_test_user,
    };
    use crate::repo::{FavoritesRepo, FolderPermissionsRepo, PhotosTransactionRepo};
    use sqlx::SqlitePool;
    use std::collections::HashMap;
    use time::macros::datetime;
    use time::{Duration, OffsetDateTime};

    const ME: &str = "alice";

    /// The ids of the test data set, by name
    struct Dataset {
        ids: HashMap<&'static str, i64>,
        shared_folder: i64,
        shared_delete_folder: i64,
        expired_folder: i64,
        unshared_folder: i64,
        own_folder: i64,
    }

    impl Dataset {
        fn id(&self, name: &str) -> i64 {
            self.ids[name]
        }

        fn names(&self, photos: &[ContextPhoto]) -> Vec<&'static str> {
            photos
                .iter()
                .map(|p| {
                    *self
                        .ids
                        .iter()
                        .find(|(_, id)| **id == p.photo.id)
                        .unwrap()
                        .0
                })
                .collect()
        }
    }

    const BIG: i64 = LARGE_FILE_MIN_SIZE + 1;

    /// alice (me), bob and carol. Photos:
    /// - own: alice's, one in her folder, one trashed, one big
    /// - public: two with equal timestamps, one trashed, one big
    /// - bob's folder shared with alice (view only), bob's folder shared with alice with
    ///   `can_delete`, bob's folder shared with alice but expired, bob's unshared folder
    /// - carol's root photo
    async fn create_dataset(pool: &SqlitePool) -> Dataset {
        for (id, name) in [("alice", "Alice"), ("bob", "Bob"), ("carol", "Carol")] {
            insert_test_user(pool, &create_test_user(id, name))
                .await
                .unwrap();
        }

        let own_folder = create_test_folder(pool, Some("alice"), "mine").await.id;
        let shared_folder = create_test_folder(pool, Some("bob"), "shared").await.id;
        let shared_delete_folder = create_test_folder(pool, Some("bob"), "shared-delete")
            .await
            .id;
        let expired_folder = create_test_folder(pool, Some("bob"), "expired").await.id;
        let unshared_folder = create_test_folder(pool, Some("bob"), "private").await.id;

        pool.create_share(shared_folder, Some("alice"), false, false, None)
            .await
            .unwrap();
        pool.create_share(
            shared_delete_folder,
            Some("alice"),
            true,
            true,
            Some(OffsetDateTime::now_utc() + Duration::days(30)),
        )
        .await
        .unwrap();
        // Expired shares can't be inserted directly (check constraint), so expire it afterwards
        let expired = pool
            .create_share(
                expired_folder,
                Some("alice"),
                true,
                true,
                Some(OffsetDateTime::now_utc() + Duration::days(1)),
            )
            .await
            .unwrap();
        sqlx::query("update folder_permissions set created_at = ?, expires_at = ? where id = ?")
            .bind(OffsetDateTime::now_utc() - Duration::days(10))
            .bind(OffsetDateTime::now_utc() - Duration::days(2))
            .bind(expired.id)
            .execute(pool)
            .await
            .unwrap();

        let trashed = Some(datetime!(2024-08-01 10:00:00 UTC));
        type Row = (
            &'static str,
            Option<&'static str>,
            Option<i64>,
            OffsetDateTime,
            i64,
            Option<OffsetDateTime>,
        );
        let rows: Vec<Row> = vec![
            (
                "own_1",
                Some("alice"),
                None,
                datetime!(2024-06-10 10:00:00 UTC),
                10,
                None,
            ),
            (
                "own_2",
                Some("alice"),
                Some(own_folder),
                datetime!(2024-05-03 09:00:00 UTC),
                20,
                None,
            ),
            (
                "own_old",
                Some("alice"),
                None,
                datetime!(2022-01-01 00:00:00 UTC),
                30,
                None,
            ),
            (
                "own_trashed",
                Some("alice"),
                None,
                datetime!(2024-06-01 10:00:00 UTC),
                40,
                trashed,
            ),
            (
                "own_big",
                Some("alice"),
                None,
                datetime!(2023-12-31 23:59:59 UTC),
                BIG + 5,
                None,
            ),
            (
                "public_a",
                None,
                None,
                datetime!(2024-06-05 12:00:00 UTC),
                50,
                None,
            ),
            (
                "public_b",
                None,
                None,
                datetime!(2024-06-05 12:00:00 UTC),
                60,
                None,
            ),
            (
                "public_trashed",
                None,
                None,
                datetime!(2024-02-01 10:00:00 UTC),
                70,
                Some(datetime!(2024-07-01 10:00:00 UTC)),
            ),
            (
                "public_big",
                None,
                None,
                datetime!(2024-03-01 10:00:00 UTC),
                BIG + 10,
                None,
            ),
            (
                "shared_1",
                Some("bob"),
                Some(shared_folder),
                datetime!(2024-06-20 10:00:00 UTC),
                80,
                None,
            ),
            (
                "shared_trashed",
                Some("bob"),
                Some(shared_folder),
                datetime!(2024-06-21 10:00:00 UTC),
                90,
                trashed,
            ),
            (
                "shared_delete_1",
                Some("bob"),
                Some(shared_delete_folder),
                datetime!(2024-04-02 10:00:00 UTC),
                BIG,
                None,
            ),
            (
                "shared_delete_trashed",
                Some("bob"),
                Some(shared_delete_folder),
                datetime!(2024-04-03 10:00:00 UTC),
                100,
                Some(datetime!(2024-09-01 10:00:00 UTC)),
            ),
            (
                "expired_1",
                Some("bob"),
                Some(expired_folder),
                datetime!(2024-06-15 10:00:00 UTC),
                BIG,
                None,
            ),
            (
                "unshared_1",
                Some("bob"),
                Some(unshared_folder),
                datetime!(2024-06-16 10:00:00 UTC),
                BIG,
                None,
            ),
            (
                "bob_root",
                Some("bob"),
                None,
                datetime!(2024-06-17 10:00:00 UTC),
                110,
                None,
            ),
            (
                "carol_root",
                Some("carol"),
                None,
                datetime!(2024-06-18 10:00:00 UTC),
                120,
                None,
            ),
        ];

        let mut ids = HashMap::new();
        let mut tx = pool.begin().await.unwrap();
        for (name, user_id, folder_id, created_at, file_size, trashed_on) in rows {
            let mut photo = create_test_photo_with_time(0, user_id, folder_id, name, created_at);
            photo.file_size = file_size;
            photo.trashed_on = trashed_on;
            let photo = tx.insert_photo(&photo).await.unwrap();
            ids.insert(name, photo.id);
        }
        tx.commit().await.unwrap();

        Dataset {
            ids,
            shared_folder,
            shared_delete_folder,
            expired_folder,
            unshared_folder,
            own_folder,
        }
    }

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

        if ctx.is_month_sectioned() {
            let mut concatenated = Vec::new();
            for summary in &summaries {
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
                assert_eq!(month_photos[0].photo.id, summary.cover_photo_id);

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
    async fn test_timeline_all(pool: SqlitePool) {
        let data = create_dataset(&pool).await;
        let ctx = PhotoListContext::Timeline(PhotoFilter::All);

        let photos = all_photos(&pool, &ctx).await;
        assert_eq!(
            data.names(&photos),
            vec![
                "shared_1",
                "own_1",
                // Equal timestamps: id desc
                "public_b",
                "public_a",
                "own_2",
                "shared_delete_1",
                "public_big",
                "own_big",
                "own_old",
            ]
        );
        check_invariants(&pool, &ctx).await;

        let not_deletable: Vec<_> = photos
            .iter()
            .filter(|p| !p.deletable)
            .map(|p| p.photo.id)
            .collect();
        assert_eq!(not_deletable, vec![data.id("shared_1")]);
    }

    #[sqlx::test]
    async fn test_timeline_filters(pool: SqlitePool) {
        let data = create_dataset(&pool).await;

        let ctx = PhotoListContext::Timeline(PhotoFilter::Personal);
        assert_eq!(
            data.names(&all_photos(&pool, &ctx).await),
            vec!["own_1", "own_2", "own_big", "own_old"]
        );
        check_invariants(&pool, &ctx).await;

        let ctx = PhotoListContext::Timeline(PhotoFilter::Family);
        assert_eq!(
            data.names(&all_photos(&pool, &ctx).await),
            vec!["public_b", "public_a", "public_big"]
        );
        check_invariants(&pool, &ctx).await;

        let ctx = PhotoListContext::Timeline(PhotoFilter::Shared);
        assert_eq!(
            data.names(&all_photos(&pool, &ctx).await),
            vec!["shared_1", "shared_delete_1"]
        );
        check_invariants(&pool, &ctx).await;

        // Bob sees his own photos as personal and nothing as shared
        let ctx = PhotoListContext::Timeline(PhotoFilter::Shared);
        assert_eq!(pool.context_count("bob", &ctx).await.unwrap(), 0);
        let ctx = PhotoListContext::Timeline(PhotoFilter::Personal);
        assert_eq!(pool.context_count("bob", &ctx).await.unwrap(), 5);
    }

    #[sqlx::test]
    async fn test_folder(pool: SqlitePool) {
        let data = create_dataset(&pool).await;

        let ctx = PhotoListContext::Folder(data.shared_folder);
        let photos = all_photos(&pool, &ctx).await;
        assert_eq!(data.names(&photos), vec!["shared_1"]);
        assert!(!photos[0].deletable);
        check_invariants(&pool, &ctx).await;

        let ctx = PhotoListContext::Folder(data.shared_delete_folder);
        let photos = all_photos(&pool, &ctx).await;
        assert_eq!(data.names(&photos), vec!["shared_delete_1"]);
        assert!(photos[0].deletable);

        let ctx = PhotoListContext::Folder(data.own_folder);
        assert_eq!(data.names(&all_photos(&pool, &ctx).await), vec!["own_2"]);
        check_invariants(&pool, &ctx).await;

        // Folder access itself is checked by `authorize`
        for folder in [data.expired_folder, data.unshared_folder] {
            assert!(matches!(
                PhotoListContext::Folder(folder).authorize(ME, &pool).await,
                Err(crate::http::error::HttpError::NotFound)
            ));
        }
        for folder in [
            data.shared_folder,
            data.shared_delete_folder,
            data.own_folder,
        ] {
            assert!(
                PhotoListContext::Folder(folder)
                    .authorize(ME, &pool)
                    .await
                    .is_ok()
            );
        }
    }

    #[sqlx::test]
    async fn test_favorites(pool: SqlitePool) {
        let data = create_dataset(&pool).await;
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
        // Someone else's favourite
        pool.insert_favorite(data.id("own_2"), "bob").await.unwrap();

        let ctx = PhotoListContext::Favorites;
        assert_eq!(
            data.names(&all_photos(&pool, &ctx).await),
            vec!["shared_1", "own_1", "public_a"]
        );
        check_invariants(&pool, &ctx).await;
    }

    #[sqlx::test]
    async fn test_trash(pool: SqlitePool) {
        let data = create_dataset(&pool).await;

        let ctx = PhotoListContext::Trash;
        let photos = all_photos(&pool, &ctx).await;
        // Oldest trashed first; equal trashed_on → id asc. `shared_trashed` has no delete access.
        assert_eq!(
            data.names(&photos),
            vec!["public_trashed", "own_trashed", "shared_delete_trashed"]
        );
        assert!(photos.iter().all(|p| p.deletable));
        check_invariants(&pool, &ctx).await;
    }

    #[sqlx::test]
    async fn test_large_files(pool: SqlitePool) {
        let data = create_dataset(&pool).await;

        let ctx = PhotoListContext::LargeFiles;
        assert_eq!(
            data.names(&all_photos(&pool, &ctx).await),
            vec!["public_big", "own_big", "shared_delete_1"]
        );
        check_invariants(&pool, &ctx).await;
    }

    #[sqlx::test]
    async fn test_position_not_in_context(pool: SqlitePool) {
        let data = create_dataset(&pool).await;

        let ctx = PhotoListContext::Timeline(PhotoFilter::All);
        for name in ["own_trashed", "unshared_1", "expired_1", "carol_root"] {
            assert_eq!(
                pool.context_position(ME, &ctx, data.id(name))
                    .await
                    .unwrap(),
                None,
                "{name}"
            );
        }
        assert_eq!(
            pool.context_position(ME, &ctx, 999_999).await.unwrap(),
            None
        );
        assert_eq!(
            pool.context_position(ME, &PhotoListContext::Trash, data.id("own_1"))
                .await
                .unwrap(),
            None
        );
    }

    #[sqlx::test]
    async fn test_memories(pool: SqlitePool) {
        insert_test_user(&pool, &create_test_user(ME, "Alice"))
            .await
            .unwrap();
        insert_test_user(&pool, &create_test_user("bob", "Bob"))
            .await
            .unwrap();

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
        let mut tx = pool.begin().await.unwrap();
        for (name, user, created_at) in rows {
            let photo = tx
                .insert_photo(&create_test_photo_with_time(
                    0, user, None, name, created_at,
                ))
                .await
                .unwrap();
            ids.insert(name, photo.id);
        }
        tx.commit().await.unwrap();

        let summary = pool.memories_summary(ME, PhotoFilter::All).await.unwrap();
        assert!(summary.windows(2).all(|w| w[0].years_ago < w[1].years_ago));
        assert!(summary.iter().all(|s| (1..=10).contains(&s.years_ago)));

        // Near new year's eve the shifted dates cross into neighbouring years (which is correct,
        // `years_ago` compares calendar years), so exact groups are only checked mid-year.
        let mid_year = (10..=355).contains(&now.ordinal());
        if mid_year {
            let groups: Vec<(u32, i64)> = summary.iter().map(|s| (s.years_ago, s.count)).collect();
            assert_eq!(groups, vec![(1, 3), (2, 1)]);
            assert_eq!(summary[0].cover_photo_id, ids["1y_plus_2d"]);
        }

        for group in &summary {
            let ctx = PhotoListContext::Memories {
                years_ago: group.years_ago,
                filter: PhotoFilter::All,
            };
            let photos = pool.context_slice(ME, &ctx, 0, 100).await.unwrap();
            assert_eq!(photos.len() as i64, group.count);
            assert_eq!(photos[0].photo.id, group.cover_photo_id);
            assert_eq!(pool.context_count(ME, &ctx).await.unwrap(), group.count);
            check_invariants(&pool, &ctx).await;
        }

        let family = pool
            .memories_summary(ME, PhotoFilter::Family)
            .await
            .unwrap();
        let shared = pool
            .memories_summary(ME, PhotoFilter::Shared)
            .await
            .unwrap();
        assert!(shared.is_empty());
        if mid_year {
            let groups: Vec<(u32, i64)> = family.iter().map(|s| (s.years_ago, s.count)).collect();
            assert_eq!(groups, vec![(1, 1), (2, 1)]);

            // The context respects the filter too
            let ctx = PhotoListContext::Memories {
                years_ago: 1,
                filter: PhotoFilter::Family,
            };
            let photos = pool.context_slice(ME, &ctx, 0, 100).await.unwrap();
            assert_eq!(photos.len(), 1);
            assert_eq!(photos[0].photo.id, ids["1y_minus_2d"]);
        }
    }

    #[sqlx::test]
    async fn test_day_of_year_window(pool: SqlitePool) {
        let within = |a: &'static str, b: &'static str| {
            let pool = pool.clone();
            async move {
                let sql = format!("select {}", day_of_year_window("?1", "?2"));
                sqlx::query_scalar::<_, bool>(sqlx::AssertSqlSafe(sql.as_str()))
                    .bind(a)
                    .bind(b)
                    .fetch_one(&pool)
                    .await
                    .unwrap()
            }
        };

        assert!(within("2025-06-10", "2021-06-10").await);
        assert!(within("2025-06-10", "2021-06-13").await);
        assert!(within("2025-06-10", "2021-06-07").await);
        assert!(!within("2025-06-10", "2021-06-14").await);
        // Wrap around the new year
        assert!(within("2025-01-02", "2020-12-30").await);
        assert!(within("2024-12-30", "2021-01-02").await);
        assert!(!within("2025-01-05", "2020-12-30").await);
    }
}
