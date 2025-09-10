use crate::http::error::{HttpError, HttpResult};
use crate::http::utils::AuthSession;
use crate::model::user::{SimpleUser, UserCredentials};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use tracing::{debug, error, warn};

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
) -> HttpResult<impl IntoResponse> {
    let valid_user = auth.authenticate(login_user).await.map_err(|e| {
        error!("Failed to authenticate: {e:?}");
        HttpError::Internal("Failed to validate credentials".to_string())
    })?;

    let Some(user) = valid_user else {
        warn!("Wrong credentials");
        return Err(HttpError::Unauthorized);
    };

    auth.login(&user).await.map_err(|e| {
        error!("Failed to login user `{}`: {}", user.id, e);
        HttpError::Internal("Failed to login".to_string())
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
