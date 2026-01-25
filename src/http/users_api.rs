use crate::http::error::HttpError;
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::utils::AuthSession;
use crate::model::user::{SimpleUser, UserCredentials};
use askama::Template;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use tracing::{debug, error, warn};

pub fn router() -> Router {
    Router::new()
        .route("/login", post(login_handler))
        .route("/logout", post(logout))
        .route("/profile", get(profile))
}

async fn login_handler(
    mut auth: AuthSession,
    headers: HeaderMap,
    Form(credentials): Form<UserCredentials>,
) -> Response {
    let wants_json = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("application/json"))
        .unwrap_or(false);

    let error_response = |message: &str| -> Response {
        if wants_json {
            HttpError::BadRequest(message.to_string()).into_response()
        } else {
            login_error(message)
        }
    };

    let internal_error = || -> Response {
        if wants_json {
            HttpError::Internal("Server encountered a problem".to_string()).into_response()
        } else {
            login_error("Server encountered a problem. Please try again later.")
        }
    };

    if credentials.user_id.is_empty() {
        return error_response("User ID is required");
    }

    if credentials.password.is_empty() {
        return error_response("Password is required");
    }

    let user = match auth.authenticate(credentials.clone()).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            warn!("Wrong credentials for user: {}", credentials.user_id);
            return if wants_json {
                HttpError::Unauthorized.into_response()
            } else {
                login_error("Invalid user ID or password")
            };
        }
        Err(e) => {
            error!(
                "Failed to authenticate user {} with error: {}",
                credentials.user_id, e
            );
            return internal_error();
        }
    };

    if let Err(e) = auth.login(&user).await {
        error!("Failed to login user {} with error: {}", user.id, e);
        return internal_error();
    }

    debug!("User has been logged in: {}", user.id);

    if wants_json {
        Json(SimpleUser::from(user)).into_response()
    } else {
        [("HX-Refresh", "true"), ("HX-Replace-Url", "/")].into_response()
    }
}

fn login_error(message: &str) -> Response {
    #[derive(Template)]
    #[template(path = "user/login_error.html")]
    struct ErrorTemplate<'a> {
        error_message: &'a str,
    }

    ErrorTemplate {
        error_message: message,
    }
    .into_response()
}

async fn profile(auth_session: AuthSession) -> impl IntoResponse {
    auth_session
        .user
        .map_or(StatusCode::UNAUTHORIZED.into_response(), |user| {
            Json(SimpleUser::from(user)).into_response()
        })
}

pub async fn logout(mut auth: AuthSession) -> String {
    if let Some(user) = &auth.user {
        debug!("Logging out user: {}", user.id);

        if let Err(e) = auth.logout().await {
            return e.to_string();
        }
    }

    "Logged out".to_string()
}
