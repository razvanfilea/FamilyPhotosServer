use crate::http::AppStateRef;
use crate::http::error::HttpResult;
use crate::http::page_context::{Nav, PageContext};
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::views::{format_bytes, plural};
use crate::repo::{CountSize, PhotoStatistics, StatsRepo};
use askama::Template;
use axum::extract::State;
use axum::response::Response;

/// A line of the statistics card
struct StatRow {
    testid: &'static str,
    icon: &'static str,
    label: &'static str,
    /// "12 photos • 3.4 MB"
    value: String,
}

fn count_size_label(stat: CountSize) -> String {
    format!(
        "{} • {}",
        plural(stat.count, "photo"),
        format_bytes(stat.size)
    )
}

/// The rows of the statistics card, in the order of the Android app
fn stat_rows(stats: &PhotoStatistics) -> Vec<StatRow> {
    [
        ("stat-family", "family_restroom", "Family", stats.family),
        ("stat-personal", "person", "Personal", stats.personal),
        ("stat-images", "image", "Images", stats.images),
        ("stat-videos", "videocam", "Videos", stats.videos),
    ]
    .into_iter()
    .map(|(testid, icon, label, stat)| StatRow {
        testid,
        icon,
        label,
        value: count_size_label(stat),
    })
    .collect()
}

#[derive(Template)]
#[template(path = "utilities/utilities_page.html")]
struct UtilitiesTemplate {
    ctx: PageContext,
    rows: Vec<StatRow>,
    total: String,
}

/// `GET /utilities`: the statistics card and the tools
pub async fn utilities_page(
    State(state): State<AppStateRef>,
    ctx: PageContext,
) -> HttpResult<Response> {
    let stats = state.read_pool.photo_statistics(&ctx.user_id).await?;

    UtilitiesTemplate {
        ctx: ctx.nav(Nav::Utilities),
        rows: stat_rows(&stats),
        total: count_size_label(stats.total),
    }
    .try_into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::api::tests::TestServer;
    use crate::repo::tests::Dataset;
    use axum::http::StatusCode;
    use sqlx::SqlitePool;

    #[test]
    fn test_count_size_label() {
        for (count, size, expected) in [
            (0, 0, "0 photos • 0 B"),
            (1, 1536, "1 photo • 1.5 KB"),
            (3, 5 * 1024 * 1024, "3 photos • 5.0 MB"),
        ] {
            assert_eq!(
                count_size_label(CountSize { count, size }),
                expected,
                "{count} {size}"
            );
        }
    }

    #[sqlx::test]
    async fn test_utilities_page(pool: SqlitePool) {
        let server = TestServer::new(pool).await;
        Dataset::create(server.pool()).await;
        let stats = server.pool().photo_statistics("alice").await.unwrap();

        let html = server
            .login("alice")
            .await
            .get("/utilities")
            .await
            .assert_status(StatusCode::OK)
            .text();

        for row in stat_rows(&stats) {
            assert!(html.contains(row.testid), "{}", row.testid);
            assert!(html.contains(&row.value), "{}: {}", row.testid, row.value);
        }
        assert!(html.contains(&count_size_label(stats.total)));
        assert!(stats.total.count > 0);
        for tool in [
            "favorites",
            "trash",
            "duplicates",
            "large-files",
            "upload",
            "settings",
        ] {
            assert!(
                html.contains(&format!("data-testid=\"tool-{tool}\"")),
                "{tool}"
            );
        }
    }
}
