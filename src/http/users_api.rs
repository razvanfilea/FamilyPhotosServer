use crate::http::error::HttpError;
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::utils::AuthSession;
use crate::model::user::{SimpleUser, UserCredentials};
use askama::Template;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use tracing::{debug, error, warn};

pub fn router() -> Router {
    Router::new()
        .route("/login", post(login_handler))
        .route("/logout", post(logout))
        .route("/profile", get(profile))
}

fn wants_html(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("text/html"))
        .unwrap_or(false)
}

async fn login_handler(
    mut auth: AuthSession,
    headers: HeaderMap,
    Form(credentials): Form<UserCredentials>,
) -> Response {
    let wants_html = wants_html(&headers);

    let error_response = |message: &str| -> Response {
        if wants_html {
            login_error(message)
        } else {
            HttpError::BadRequest(message.to_string()).into_response()
        }
    };

    let internal_error = || -> Response {
        if wants_html {
            login_error("Server encountered a problem. Please try again later.")
        } else {
            HttpError::Internal("Server encountered a problem".to_string()).into_response()
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
            return if wants_html {
                login_error("Invalid user ID or password")
            } else {
                HttpError::Unauthorized.into_response()
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

    if wants_html {
        [("HX-Refresh", "true"), ("HX-Replace-Url", "/")].into_response()
    } else {
        Json(SimpleUser::from(user)).into_response()
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

pub async fn logout(mut auth: AuthSession, headers: HeaderMap) -> Response {
    let wants_html = wants_html(&headers);

    if let Some(user) = &auth.user {
        debug!("Logging out user: {}", user.id);
    }

    if auth.user.is_some()
        && let Err(e) = auth.logout().await
    {
        error!("Failed to logout: {}", e);
        return HttpError::Internal("Failed to logout".to_string()).into_response();
    }

    if wants_html {
        Redirect::to("/").into_response()
    } else {
        Json(serde_json::json!({"message": "Logged out"})).into_response()
    }
}
