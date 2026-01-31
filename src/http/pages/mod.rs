use crate::http::AppStateRef;
use crate::repo::users_repo::UsersRepository;
use axum::Router;
use axum::routing::{get, post};
use axum_login::login_required;

mod favorites;
mod folders;
mod gallery;
mod timeline;
mod trash;
mod upload;
mod user;

pub fn router(app_state: AppStateRef) -> Router {
    let authenticated_router = Router::new()
        .route("/", get(gallery::gallery_page))
        .route("/gallery/grid", get(gallery::photo_grid))
        .route("/gallery/more", get(gallery::load_more_gallery))
        .route("/folder/{folder_name}", get(gallery::folder_page))
        .route("/folder/{folder_name}/more", get(gallery::load_more_folder))
        .route("/photo/{photo_id}", get(gallery::photo_modal))
        .route(
            "/photo/{photo_id}/info-panel",
            get(gallery::photo_info_panel),
        )
        .route("/photo/{photo_id}/viewer", get(gallery::photo_viewer_media))
        .route("/folders", get(folders::folders_page))
        .route("/api/folders", get(folders::folders_list_json))
        .route("/favorites", get(favorites::favorites_page))
        .route("/favorites/more", get(favorites::load_more_favorites))
        .route(
            "/favorite/{photo_id}",
            post(favorites::toggle_favorite).delete(favorites::toggle_favorite),
        )
        .route("/trash", get(trash::trash_page))
        .route("/trash/restore/{photo_id}", post(trash::restore_photo))
        .route(
            "/trash/{photo_id}",
            post(trash::trash_photo).delete(trash::permanent_delete),
        )
        .route("/upload", get(upload::upload_page))
        .route_layer(login_required!(UsersRepository, login_url = "/login"));

    let unauthenticated_router = Router::new().route("/login", get(user::login_page));

    Router::new()
        .merge(authenticated_router)
        .merge(unauthenticated_router)
        .with_state(app_state)
}
