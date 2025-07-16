use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use tracing::{debug, error};

use crate::http::utils::{AuthSession, AxumResult};
use crate::model::user::{SimpleUser, UserCredentials};

pub fn router() -> Router {
    Router::new()
        .route("/profile", get(profile))
        .route("/login", post(login))
        .route("/logout", post(logout))
}

async fn profile(auth_session: AuthSession) -> impl IntoResponse {
    auth_session
        .user
        .map_or(StatusCode::UNAUTHORIZED.into_response(), |user| {
            Json(SimpleUser::from(user)).into_response()
        })
}

async fn login(
    mut auth: AuthSession,
    Form(login_user): Form<UserCredentials>,
) -> AxumResult<impl IntoResponse> {
    let valid_user = auth.authenticate(login_user).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to validate credentials: {e}"),
        )
    })?;

    let user = match valid_user {
        None => {
            return Err((StatusCode::UNAUTHORIZED, "Wrong user name or password")
                .into_response()
                .into());
        }
        Some(user) => user,
    };

    auth.login(&user).await.map_err(|e| {
        error!("Failed to login user with {}: {}", user.id, e);
        (StatusCode::INTERNAL_SERVER_ERROR, "Failed to login")
    })?;

    Ok(Json(SimpleUser::from(user)))
}

async fn logout(mut auth: AuthSession) -> String {
    if let Some(user) = &auth.user {
        debug!("Logging out user: {}", user.id);

        if let Err(e) = auth.logout().await {
            return e.to_string();
        }
    }

    "Failed to log out".to_string()
}
