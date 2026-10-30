use crate::http::error::HttpResult;
use crate::http::page_context::{Nav, PageContext};
use crate::http::pages::folders::sort_label;
use crate::http::prefs::FolderSort;
use crate::http::template_into_response::TemplateIntoResponse;
use askama::Template;
use axum::response::Response;

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
