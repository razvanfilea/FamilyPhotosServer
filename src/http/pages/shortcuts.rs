use crate::http::error::HttpResult;
use crate::http::template_into_response::TemplateIntoResponse;
use askama::Template;
use axum::response::Response;

#[derive(Template)]
#[template(path = "dialogs/shortcuts.html")]
struct ShortcutsDialogTemplate;

pub async fn shortcuts_dialog() -> HttpResult<Response> {
    ShortcutsDialogTemplate.try_into_response()
}
