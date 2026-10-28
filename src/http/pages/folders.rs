use crate::http::error::HttpResult;
use crate::http::page_context::{Nav, PageContext};
use crate::http::pages::errors::{not_found, not_found_page};
use crate::http::pages::placeholder::placeholder_page;
use crate::http::prefs::Prefs;
use crate::http::utils::AuthSession;
use axum::extract::Path;
use axum::response::{IntoResponse, Redirect, Response};

pub async fn folders_page(ctx: PageContext) -> HttpResult<Response> {
    placeholder_page(ctx.nav(Nav::Folders), "Folders", "folder")
}

pub async fn folder_page(ctx: PageContext, Path(folder_id): Path<String>) -> HttpResult<Response> {
    if folder_id.parse::<i64>().is_err() {
        return Ok(not_found_page(ctx));
    }
    placeholder_page(ctx.nav(Nav::Folders), "Folder", "folder")
}

/// Old bookmarks used `/folder/{id}`
pub async fn legacy_folder_redirect(
    auth: AuthSession,
    prefs: Prefs,
    Path(folder_id): Path<String>,
) -> Response {
    match folder_id.parse::<i64>() {
        Ok(id) => Redirect::permanent(&format!("/folders/{id}")).into_response(),
        Err(_) => not_found(auth, prefs).await,
    }
}
