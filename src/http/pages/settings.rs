use crate::http::AppStateRef;
use crate::http::api::users::change_password_for;
use crate::http::auth::AuthenticatedUser;
use crate::http::error::{HttpError, HttpResult};
use crate::http::fragments::{ToastKind, toast_trigger};
use crate::http::page_context::{Nav, PageContext};
use crate::http::pages::folders::sort_label;
use crate::http::prefs::FolderSort;
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::utils::AuthSession;
use askama::Template;
use axum::Form;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

/// Columns of the preview next to each zoom level (the phone column counts of the grid)
const ZOOM_PREVIEW_COLUMNS: [u8; 4] = [3, 4, 5, 7];

/// A grid size option with the preview of its columns
struct ZoomOption {
    level: usize,
    columns: u8,
    checked: bool,
}

#[derive(Template)]
#[template(path = "settings/settings_page.html")]
struct SettingsTemplate {
    ctx: PageContext,
    /// `(value, label)` of the default photo filter
    filters: [(&'static str, &'static str); 4],
    /// `(value, label)` of the folder sort orders
    folder_sorts: Vec<(&'static str, &'static str)>,
    zoom_options: Vec<ZoomOption>,
    version: &'static str,
}

/// `GET /settings`: the account, the display preferences, the password and the version
pub async fn settings_page(ctx: PageContext) -> HttpResult<Response> {
    let folder_sorts = [
        FolderSort::NameAsc,
        FolderSort::NameDesc,
        FolderSort::DateDesc,
        FolderSort::CountDesc,
    ]
    .into_iter()
    .map(|sort| (sort.as_str(), sort_label(sort)))
    .collect();

    let zoom_options = ZOOM_PREVIEW_COLUMNS
        .into_iter()
        .enumerate()
        .map(|(level, columns)| ZoomOption {
            level,
            columns,
            checked: level == usize::from(ctx.prefs.zoom),
        })
        .collect();

    SettingsTemplate {
        zoom_options,
        ctx: ctx.nav(Nav::Utilities),
        filters: [
            ("all", "All"),
            ("personal", "Personal"),
            ("family", "Family"),
            ("shared", "Shared"),
        ],
        folder_sorts,
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
    use crate::http::pages::folder_tests::{App, PASSWORD, triggers};
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode, header};
    use sqlx::SqlitePool;

    #[sqlx::test]
    async fn test_ui_change_password(pool: SqlitePool) {
        let app = App::new(pool).await;
        let carol = app.login("carol").await;
        let uri = "/ui/settings/password";
        let body = |current: &str, new: &str, confirm: &str| {
            format!("current_password={current}&new_password={new}&confirm_password={confirm}")
        };

        for (form, message, field) in [
            (
                body(PASSWORD, "short", "short"),
                "Password must be at least 8 characters",
                "new_password",
            ),
            (
                body(PASSWORD, "long-enough-1", "long-enough-2"),
                "The passwords don&#39;t match",
                "confirm_password",
            ),
            (
                body("wrong", "long-enough-1", "long-enough-1"),
                "Current password is incorrect",
                "current_password",
            ),
        ] {
            let response = app.form(&carol, Method::POST, uri, &form).await;
            assert_eq!(
                response.status,
                StatusCode::UNPROCESSABLE_ENTITY,
                "{message}"
            );
            assert!(response.body.contains("data-slot=\"password-form\""));
            assert!(response.body.contains(message), "{}", response.body);
            assert!(response.body.contains(&format!(r#"data-field="{field}""#)));
        }

        let response = app
            .form(
                &carol,
                Method::POST,
                uri,
                &body(PASSWORD, "long-enough-1", "long-enough-1"),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "{}", response.body);
        assert_eq!(
            triggers(&response)["fp:toast"]["message"],
            "Password changed"
        );
        assert!(response.body.contains("data-slot=\"password-form\""));
        assert!(!response.body.contains("long-enough-1"));

        // The new password works, the old one doesn't
        let login = |password: &str| {
            app.send(
                Request::post("/api/login")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(format!("user_id=carol&password={password}")))
                    .unwrap(),
            )
        };
        assert_eq!(login("long-enough-1").await.status, StatusCode::OK);
        assert_eq!(login(PASSWORD).await.status, StatusCode::UNAUTHORIZED);
    }
}
