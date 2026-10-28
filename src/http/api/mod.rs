mod context;
mod download;
mod favorite;
mod folders;
mod move_photos;
pub mod photos_info;
mod photos_misc;
mod sharing;
mod sync;
mod trash;
mod upload;
mod users;

#[cfg(test)]
pub(crate) mod tests;

use axum::Router;
use axum::http::StatusCode;
use axum_login::login_required;

use crate::http::AppStateRef;
use crate::repo::users_repo::UsersRepository;

pub fn router(app_state: AppStateRef) -> Router {
    let protected_router = Router::new()
        .nest("/sync", sync::router())
        .nest("/context", context::router())
        .nest("/move", move_photos::router())
        .nest("/trash", trash::router())
        .nest("/sharing", sharing::router())
        .nest("/favorite", favorite::router())
        .nest("/folders", folders::router())
        .nest("/upload", upload::router())
        .merge(photos_misc::router())
        .merge(photos_info::router())
        .merge(download::router())
        .merge(users::protected_router())
        .route_layer(login_required!(UsersRepository));

    Router::new()
        .merge(protected_router)
        .merge(users::public_router())
        .fallback(async || StatusCode::NOT_FOUND)
        .with_state(app_state)
}
