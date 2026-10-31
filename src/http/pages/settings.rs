use crate::http::AppStateRef;
use crate::http::api::users::change_password_for;
use crate::http::auth::AuthenticatedUser;
use crate::http::error::{HttpError, HttpResult};
use crate::http::fragments::{ToastKind, toast_trigger};
use crate::http::page_context::{Nav, PageContext};
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::utils::AuthSession;
use askama::Template;
use axum::Form;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

#[derive(Template)]
#[template(path = "settings/settings_page.html")]
struct SettingsTemplate {
    ctx: PageContext,
    version: &'static str,
}

/// `GET /settings`: the account, the password and the version
pub async fn settings_page(ctx: PageContext) -> HttpResult<Response> {
    SettingsTemplate {
        ctx: ctx.nav(Nav::Utilities),
        version: env!("CARGO_PKG_VERSION"),
    }
    .try_into_response()
}

#[derive(Template)]
#[template(path = "settings/password_form.html")]
struct PasswordFormTemplate {
    error: Option<String>,
    /// Name of the input the error is about
    field: &'static str,
}

#[derive(Deserialize)]
pub struct PasswordForm {
    current_password: String,
    new_password: String,
    confirm_password: String,
}

/// `POST /ui/settings/password`: changes the password; the answer is the form again (empty, or
/// with the reason for `hx-status:422`)
pub async fn change_password(
    auth: AuthSession,
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppStateRef>,
    Form(form): Form<PasswordForm>,
) -> HttpResult<Response> {
    let result = if form.new_password != form.confirm_password {
        Err(HttpError::BadRequest(
            "The passwords don't match".to_string(),
        ))
    } else {
        change_password_for(
            state,
            &auth,
            &user,
            form.current_password,
            form.new_password,
        )
        .await
    };

    match result {
        Ok(()) => {
            let body = PasswordFormTemplate {
                error: None,
                field: "",
            }
            .try_into_response()?;
            Ok((toast_trigger("Password changed", ToastKind::Success), body).into_response())
        }
        Err(HttpError::BadRequest(message)) => {
            // The field to put the cursor in
            let field = if message.starts_with("The passwords") {
                "confirm_password"
            } else if message.starts_with("Current") {
                "current_password"
            } else {
                "new_password"
            };
            let body = PasswordFormTemplate {
                error: Some(message),
                field,
            }
            .try_into_response()?;
            Ok((StatusCode::UNPROCESSABLE_ENTITY, body).into_response())
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use crate::http::api::tests::{PASSWORD, TestServer};
    use axum::http::{Method, StatusCode};
    use sqlx::SqlitePool;

    #[sqlx::test]
    async fn test_ui_change_password(pool: SqlitePool) {
        let server = TestServer::new(pool).await;
        let carol = server.login("carol").await;
        let change = async |current: &str, new: &str, confirm: &str| {
            let body =
                format!("current_password={current}&new_password={new}&confirm_password={confirm}");
            carol
                .form(Method::POST, "/ui/settings/password", &body)
                .await
        };

        for (current, new, confirm, message, field) in [
            (
                PASSWORD,
                "short",
                "short",
                "Password must be at least 8 characters",
                "new_password",
            ),
            (
                PASSWORD,
                "long-enough-1",
                "long-enough-2",
                "The passwords don&#39;t match",
                "confirm_password",
            ),
            (
                "wrong",
                "long-enough-1",
                "long-enough-1",
                "Current password is incorrect",
                "current_password",
            ),
        ] {
            let response = change(current, new, confirm).await;
            let html = response
                .assert_status(StatusCode::UNPROCESSABLE_ENTITY)
                .text();
            assert!(html.contains(r#"data-slot="password-form""#), "{message}");
            assert!(html.contains(message), "{html}");
            assert!(
                html.contains(&format!(r#"data-field="{field}""#)),
                "{message}"
            );
        }

        let response = change(PASSWORD, "long-enough-1", "long-enough-1").await;
        let response = response.assert_status(StatusCode::OK);
        assert_eq!(
            response.triggers()["fp:toast"]["message"],
            "Password changed"
        );
        let html = response.text();
        assert!(html.contains(r#"data-slot="password-form""#) && !html.contains("long-enough-1"));

        // The new password works, the old one doesn't
        let login = async |password: &str| {
            let form = format!("user_id=carol&password={password}");
            server.login_with(&form, &[]).await.status
        };
        assert_eq!(login("long-enough-1").await, StatusCode::OK);
        assert_eq!(login(PASSWORD).await, StatusCode::UNAUTHORIZED);
    }
}
