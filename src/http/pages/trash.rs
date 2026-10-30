use crate::http::AppStateRef;
use crate::http::auth::AuthenticatedUser;
use crate::http::context::PhotoListContext;
use crate::http::error::HttpResult;
use crate::http::page_context::{Nav, PageContext};
use crate::http::pages::grid::{GridView, build_grid_view};
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::views::plural;
use crate::repo::{CONTEXT_IDS_LIMIT, ContextPhoto, ContextRepo};
use askama::Template;
use axum::Json;
use axum::extract::State;
use axum::response::Response;

#[derive(Template)]
#[template(path = "trash/trash_page.html")]
struct TrashTemplate {
    ctx: PageContext,
    count_label: String,
    grid: GridView,
}

/// `GET /trash`: the trashed photos the user can delete, oldest first
pub async fn trash_page(
    State(state): State<AppStateRef>,
    ctx: PageContext,
) -> HttpResult<Response> {
    let context = PhotoListContext::Trash;
    let grid = build_grid_view(state, &ctx.user_id, &context, &ctx.prefs).await?;

    TrashTemplate {
        ctx: ctx.nav(Nav::Trash).viewer_context(&context),
        count_label: plural(grid.total, "photo"),
        grid,
    }
    .try_into_response()
}

/// The ids of the photos `/api/delete/{id}` accepts: the user's own and the public ones. Photos
/// of other users' shared folders can be restored from the trash, but not deleted for good.
/// The same rule is in `summarize` of `assets/js/selection/math.js` (for the selection bar).
fn permanently_deletable_ids(me: &str, photos: &[ContextPhoto]) -> Vec<i64> {
    photos
        .iter()
        .map(|context_photo| &context_photo.photo)
        .filter(|photo| photo.user_id.as_deref().is_none_or(|owner| owner == me))
        .map(|photo| photo.id)
        .collect()
}

/// `GET /ui/trash/deletable-ids`: what "Empty trash" deletes, as a JSON array (at most
/// `CONTEXT_IDS_LIMIT` ids, oldest first)
pub async fn deletable_ids(
    State(state): State<AppStateRef>,
    AuthenticatedUser(user): AuthenticatedUser,
) -> HttpResult<Json<Vec<i64>>> {
    let photos = state
        .read_pool
        .context_slice(&user.id, &PhotoListContext::Trash, 0, CONTEXT_IDS_LIMIT)
        .await?;

    Ok(Json(permanently_deletable_ids(&user.id, &photos)))
}

#[cfg(test)]
mod tests {
    use crate::http::api::tests::TestServer;
    use crate::repo::tests::Dataset;
    use axum::http::StatusCode;
    use serde_json::Value;
    use sqlx::SqlitePool;

    #[sqlx::test]
    async fn test_trash_page(pool: SqlitePool) {
        let server = TestServer::new(pool).await;
        Dataset::create(server.pool()).await;
        let alice = server.login("alice").await;

        let html = alice
            .get("/trash")
            .await
            .assert_status(StatusCode::OK)
            .text();
        assert!(html.contains("data-context=\"trash\""));
        assert!(html.contains("data-testid=\"trash-empty\""));
        assert!(html.contains("automatically deleted after 30 days"));

        // The viewer opens in the trash context, also for deep links
        let html = alice
            .get("/trash?photo=1")
            .await
            .assert_status(StatusCode::OK)
            .text();
        assert!(html.contains("data-initial-photo=\"1\""));
        assert!(html.contains("data-context=\"trash\""));
    }

    /// "Empty trash" deletes what `/api/delete` accepts: own and public photos, not the ones of
    /// other users' shared folders (which alice can restore, though)
    #[sqlx::test]
    async fn test_deletable_ids(pool: SqlitePool) {
        let server = TestServer::new(pool).await;
        let data = Dataset::create(server.pool()).await;
        let alice = server.login("alice").await;

        let response = alice
            .get("/ui/trash/deletable-ids")
            .await
            .assert_status(StatusCode::OK);
        let ids: Vec<i64> = serde_json::from_str::<Value>(&response.text())
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_i64().unwrap())
            .collect();
        let mut names = data.names(ids);
        names.sort();
        assert_eq!(names, ["own_trashed", "public_trashed"]);

        // The context itself also lists the trashed photo of the shared folder with can_delete
        let context = alice
            .get("/api/context/ids?ctx=trash")
            .await
            .assert_status(StatusCode::OK);
        assert!(
            context
                .text()
                .contains(&data.id("shared_delete_trashed").to_string())
        );

        server
            .anonymous()
            .get("/ui/trash/deletable-ids")
            .await
            .assert_status(StatusCode::UNAUTHORIZED);
    }
}
