use crate::http::AppStateRef;
use crate::http::auth::AuthenticatedUser;
use crate::http::error::{HttpError, HttpResult};
use crate::http::fragments::safe_redirect_path;
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::utils::{AuthSession, WantsHtml};
use crate::model::user::{SimpleUser, User, UserCredentials};
use crate::utils::password_hash::{generate_hash_from_password, validate_credentials};
use askama::Template;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use serde::Deserialize;
use tokio::task;
use tracing::{debug, error, warn};

pub fn public_router() -> Router<AppStateRef> {
    Router::new()
        .route("/login", post(login_handler))
        .route("/logout", post(logout))
}

pub fn protected_router() -> Router<AppStateRef> {
    Router::new()
        .route("/profile", get(profile))
        .route("/profile/password", post(change_password))
        .route("/members", get(members_list))
}

#[derive(Deserialize)]
struct LoginForm {
    user_id: String,
    password: String,
    /// Web only: where to redirect after logging in
    next: Option<String>,
}

async fn login_handler(
    auth: AuthSession,
    WantsHtml(wants_html): WantsHtml,
    Form(form): Form<LoginForm>,
) -> Response {
    // User ids are always lowercase; the Android app normalizes them the same way
    let credentials = UserCredentials {
        user_id: form.user_id.trim().to_lowercase(),
        password: form.password,
    };

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
                login_error("Invalid username or password.")
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
        let next = form
            .next
            .as_deref()
            .and_then(safe_redirect_path)
            .and_then(|next| HeaderValue::from_str(next).ok())
            .unwrap_or(HeaderValue::from_static("/"));
        [("HX-Redirect", next)].into_response()
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
        .user()
        .await
        .map_or(StatusCode::UNAUTHORIZED.into_response(), |user| {
            Json(SimpleUser::from(user)).into_response()
        })
}

const MIN_PASSWORD_LENGTH: usize = 8;
const MAX_PASSWORD_LENGTH: usize = 1024;

#[derive(Deserialize)]
struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
}

/// Web only. Other sessions of the user end (axum-login ties a session to the password hash); the
/// current one is logged in again with the new hash.
async fn change_password(
    State(state): State<AppStateRef>,
    auth: AuthSession,
    AuthenticatedUser(user): AuthenticatedUser,
    Json(request): Json<ChangePasswordRequest>,
) -> HttpResult<impl IntoResponse> {
    change_password_for(
        state,
        &auth,
        &user,
        request.current_password,
        request.new_password,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Checks the current password and sets the new one. Shared with the web UI's settings form.
pub(crate) async fn change_password_for(
    state: AppStateRef,
    auth: &AuthSession,
    user: &User,
    current_password: String,
    new_password: String,
) -> HttpResult<()> {
    let current_hash = user.password_hash.clone();
    let is_correct =
        task::spawn_blocking(move || validate_credentials(current_password, current_hash))
            .await
            .map_err(|e| HttpError::Internal(e.to_string()))?
            .map_err(|e| HttpError::Internal(e.to_string()))?;
    if !is_correct {
        return Err(HttpError::BadRequest(
            "Current password is incorrect".to_string(),
        ));
    }

    let length = new_password.chars().count();
    if length < MIN_PASSWORD_LENGTH {
        return Err(HttpError::BadRequest(format!(
            "Password must be at least {MIN_PASSWORD_LENGTH} characters"
        )));
    }
    if length > MAX_PASSWORD_LENGTH {
        return Err(HttpError::BadRequest("Password is too long".to_string()));
    }

    let new_hash = task::spawn_blocking(move || generate_hash_from_password(new_password))
        .await
        .map_err(|e| HttpError::Internal(e.to_string()))?;
    state
        .users_repo
        .update_password(&user.id, &new_hash)
        .await?;

    let updated = state
        .users_repo
        .get_user(&user.id)
        .await
        .ok_or_else(|| HttpError::Internal("User disappeared".to_string()))?;
    auth.login(&updated)
        .await
        .map_err(|e| HttpError::Internal(e.to_string()))?;

    debug!("Password changed for user: {}", user.id);
    Ok(())
}

pub async fn logout(auth: AuthSession, WantsHtml(wants_html): WantsHtml) -> Response {
    let user = auth.user().await;
    if let Some(user) = &user {
        debug!("Logging out user: {}", user.id);
    }

    if user.is_some()
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

async fn members_list(
    State(state): State<AppStateRef>,
    AuthenticatedUser(_user): AuthenticatedUser,
) -> HttpResult<impl IntoResponse> {
    let users = state.users_repo.get_users().await?;
    let members: Vec<SimpleUser> = users.into_iter().map(SimpleUser::from).collect();
    Ok(Json(members))
}
