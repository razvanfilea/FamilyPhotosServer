mod download;
mod favorite;
mod move_photos;
mod photos;
mod sharing;
mod sync;
mod trash;
mod upload;
mod users;

use axum::Router;
use axum_login::login_required;

use crate::http::AppStateRef;
use crate::repo::users_repo::UsersRepository;

pub fn router(app_state: AppStateRef) -> Router {
    let protected_router = Router::new()
        .nest("/sync", sync::router())
        .nest("/move", move_photos::router())
        .nest("/trash", trash::router())
        .nest("/sharing", sharing::router())
        .nest("/favorite", favorite::router())
        .merge(photos::router())
        .merge(upload::router())
        .merge(download::router())
        .merge(users::protected_router())
        .route_layer(login_required!(UsersRepository));

    Router::new()
        .merge(protected_router)
        .merge(users::public_router())
        .with_state(app_state)
}
