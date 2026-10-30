use crate::http::AppStateRef;
use crate::http::UPLOAD_CONTENT_SECURITY_POLICY;
use crate::http::fragments;
use crate::repo::users_repo::UsersRepository;
use axum::Router;
use axum::http::{HeaderValue, header};
use axum::middleware;
use axum::routing::get;
use axum_login::login_required;
use tower_http::set_header::SetResponseHeaderLayer;

mod destination_picker;
mod duplicates;
mod errors;
mod favorites;
mod folders;
pub mod grid;
mod large_files;
mod settings;
mod sharing;
mod shortcuts;
mod timeline;
mod trash;
mod upload;
mod user;
mod utilities;
mod viewer;

pub use errors::not_found;

#[cfg(test)]
mod folder_tests;

pub fn router(app_state: AppStateRef) -> Router {
    // Full pages: redirect to the login page when logged out
    let pages_router = Router::new()
        .route("/", get(timeline::timeline_page))
        .route("/folders", get(folders::folders_page))
        .route("/folders/{folder_id}", get(folders::folder_page))
        .route("/favorites", get(favorites::favorites_page))
        .route("/utilities", get(utilities::utilities_page))
        .route("/trash", get(trash::trash_page))
        .route("/duplicates", get(duplicates::duplicates_page))
        .route("/large-files", get(large_files::large_files_page))
        .route("/settings", get(settings::settings_page))
        .route(
            "/upload",
            get(upload::upload_page).layer(SetResponseHeaderLayer::overriding(
                header::CONTENT_SECURITY_POLICY,
                HeaderValue::from_static(UPLOAD_CONTENT_SECURITY_POLICY),
            )),
        )
        .route_layer(login_required!(UsersRepository, login_url = "/login"));

    // HTML fragments for HTMX/JS: 401 + HX-Redirect when logged out
    let fragments_router = Router::new()
        .route("/ui/dialogs/shortcuts", get(shortcuts::shortcuts_dialog))
        .route("/ui/folders/list", get(folders::list_fragment))
        .route("/ui/dialogs/folder-name", get(folders::folder_name_dialog))
        .route(
            "/ui/dialogs/sharing/{folder_id}",
            get(sharing::sharing_dialog),
        )
        .route(
            "/ui/destination-picker",
            get(destination_picker::picker_fragment),
        )
        .route("/ui/dialogs/move", get(destination_picker::move_dialog))
        .route("/ui/grid/month", get(grid::month_page))
        .route("/ui/grid/month-picker", get(grid::month_picker))
        .route("/ui/viewer/info/{photo_id}", get(viewer::info_panel))
        .route("/ui/trash/deletable-ids", get(trash::deletable_ids))
        .route_layer(middleware::from_fn(fragments::require_login));

    let unauthenticated_router = Router::new()
        .route("/login", get(user::login_page))
        .route("/folder/{folder_id}", get(folders::legacy_folder_redirect));

    Router::new()
        .merge(pages_router)
        .merge(fragments_router)
        .merge(unauthenticated_router)
        .with_state(app_state)
}
