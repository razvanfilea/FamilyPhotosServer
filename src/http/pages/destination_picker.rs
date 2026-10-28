//! The destination picker: where moved or uploaded photos go (see `assets/js/destination-picker`)

use crate::http::AppStateRef;
use crate::http::auth::AuthenticatedUser;
use crate::http::error::HttpResult;
use crate::http::pages::folders::{BrowserOptions, BrowserParams, FolderBrowser, folder_type_info};
use crate::http::prefs::Prefs;
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::views::plural;
use crate::repo::FoldersRepo;
use askama::Template;
use axum::extract::{Query, State};
use axum::response::Response;
use serde::Deserialize;

/// A folder that is selected when the picker opens
#[derive(Debug, Clone)]
struct InitialFolder {
    id: i64,
    name: String,
    is_public: bool,
    type_key: &'static str,
    type_label: &'static str,
}

#[derive(Debug, Clone)]
struct PickerView {
    /// `move` or `upload`
    mode: &'static str,
    initial_folder: Option<InitialFolder>,
}

#[derive(Template)]
#[template(path = "destination_picker/picker.html")]
struct PickerTemplate {
    picker: PickerView,
    /// Lists the folders the user can upload to
    browser: FolderBrowser,
}

#[derive(Template)]
#[template(path = "destination_picker/move_dialog.html")]
struct MoveDialogTemplate {
    title: String,
    picker: PickerView,
    browser: FolderBrowser,
}

#[derive(Debug, Deserialize)]
pub struct PickerQuery {
    mode: Option<String>,
    /// Preselect this folder (if the user can upload to it)
    folder_id: Option<i64>,
}

async fn picker_view(
    state: AppStateRef,
    user_id: &str,
    options: BrowserOptions,
    query: &PickerQuery,
) -> HttpResult<(PickerView, FolderBrowser)> {
    let cards = state.read_pool.get_folder_cards(user_id).await?;

    let initial_folder = query
        .folder_id
        .and_then(|id| cards.iter().find(|card| card.id == id && card.can_upload))
        .map(|card| {
            let (type_key, type_label, _) = folder_type_info(card.folder_type);
            InitialFolder {
                id: card.id,
                name: card.name.clone(),
                is_public: card.owner_id.is_none(),
                type_key,
                type_label,
            }
        });

    let picker = PickerView {
        mode: if query.mode.as_deref() == Some("upload") {
            "upload"
        } else {
            "move"
        },
        initial_folder,
    };
    Ok((
        picker,
        FolderBrowser::new(cards, options, String::new(), true),
    ))
}

/// `GET /ui/destination-picker?mode=move|upload&folder_id=`
pub async fn picker_fragment(
    AuthenticatedUser(user): AuthenticatedUser,
    prefs: Prefs,
    State(state): State<AppStateRef>,
    Query(params): Query<BrowserParams>,
    Query(query): Query<PickerQuery>,
) -> HttpResult<Response> {
    let (picker, browser) = picker_view(state, &user.id, params.resolve(&prefs), &query).await?;
    PickerTemplate { picker, browser }.try_into_response()
}

#[derive(Debug, Deserialize)]
pub struct MoveDialogQuery {
    count: Option<i64>,
    folder_id: Option<i64>,
}

/// `GET /ui/dialogs/move?count=N`: the picker in a dialog with a "Move photos" button
pub async fn move_dialog(
    AuthenticatedUser(user): AuthenticatedUser,
    prefs: Prefs,
    State(state): State<AppStateRef>,
    Query(params): Query<BrowserParams>,
    Query(query): Query<MoveDialogQuery>,
) -> HttpResult<Response> {
    let picker_query = PickerQuery {
        mode: None,
        folder_id: query.folder_id,
    };
    let title = match query.count {
        Some(count) if count > 0 => format!("Move {}", plural(count, "photo")),
        _ => "Move photos".to_string(),
    };

    let (picker, browser) =
        picker_view(state, &user.id, params.resolve(&prefs), &picker_query).await?;
    MoveDialogTemplate {
        title,
        picker,
        browser,
    }
    .try_into_response()
}
