use crate::http::AppStateRef;
use crate::http::auth::AuthenticatedUser;
use crate::http::context::PhotoListContext;
use crate::http::error::{HttpError, HttpResult};
use crate::http::page_context::{Nav, PageContext};
use crate::http::pages::errors::{not_found, not_found_page};
use crate::http::pages::grid::{GridView, build_grid_view};
use crate::http::prefs::{FolderSort, FolderView, Prefs};
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::utils::AuthSession;
use crate::http::views::plural;
use crate::model::photo_filter::PhotoFilter;
use crate::repo::{FolderCard, FolderType, FoldersRepo};
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;

/// A folder as the cards of the folder list show it
#[derive(Debug, Clone)]
pub struct FolderCardView {
    pub id: i64,
    pub name: String,
    /// `family`, `personal` or `shared`
    pub type_key: &'static str,
    pub type_label: &'static str,
    pub type_icon: &'static str,
    pub is_public: bool,
    pub cover_photo_id: Option<i64>,
    /// Standard base64 of the cover's thumbhash, empty if it has none
    pub thumb_hash_b64: String,
    /// "3 photos"
    pub count_label: String,
    /// "3 photos • Personal" (+ the owner for shared folders), shown in the list view
    pub list_details: String,
}

impl FolderCardView {
    pub fn new(card: &FolderCard) -> Self {
        let (type_key, type_label, type_icon) = folder_type_info(card.folder_type);
        let count_label = plural(card.photo_count, "photo");

        let mut list_details = format!("{count_label} • {type_label}");
        if card.folder_type == FolderType::Shared
            && let Some(owner) = &card.owner_name
        {
            list_details.push_str(" • ");
            list_details.push_str(owner);
        }

        Self {
            id: card.id,
            name: card.name.clone(),
            type_key,
            type_label,
            type_icon,
            is_public: card.owner_id.is_none(),
            cover_photo_id: card.cover_photo_id,
            thumb_hash_b64: card
                .cover_thumb_hash
                .as_ref()
                .map(|hash| STANDARD.encode(hash))
                .unwrap_or_default(),
            count_label,
            list_details,
        }
    }
}

/// `(key, label, icon)` of a folder type
pub fn folder_type_info(folder_type: FolderType) -> (&'static str, &'static str, &'static str) {
    match folder_type {
        FolderType::Family => ("family", "Family", "family_restroom"),
        FolderType::Personal => ("personal", "Personal", "person"),
        FolderType::Shared => ("shared", "Shared", "group"),
    }
}

/// Everything the folder browser markup (`templates/folders/*.html`) renders, on the Folders tab
/// and in the destination picker
#[derive(Debug, Clone)]
pub struct FolderBrowser {
    pub filter: &'static str,
    pub sort: &'static str,
    pub sort_label: &'static str,
    pub view: &'static str,
    /// The search query, to restore the search field. Searching itself happens in the browser.
    pub query: String,
    /// Cards are selectable buttons instead of links
    pub picker: bool,
    pub cards: Vec<FolderCardView>,
}

impl FolderBrowser {
    pub fn new(
        cards: Vec<FolderCard>,
        options: BrowserOptions,
        query: String,
        picker: bool,
    ) -> Self {
        let cards = filter_and_sort(cards, options.filter, options.sort, picker);
        Self {
            filter: options.filter.as_str(),
            sort: options.sort.as_str(),
            sort_label: sort_label(options.sort),
            view: options.view.as_str(),
            query,
            picker,
            cards: cards.iter().map(FolderCardView::new).collect(),
        }
    }
}

pub fn sort_label(sort: FolderSort) -> &'static str {
    match sort {
        FolderSort::DateDesc => "Date (Newest)",
        FolderSort::NameAsc => "Name (A to Z)",
        FolderSort::NameDesc => "Name (Z to A)",
        FolderSort::CountDesc => "Photo Count (Most)",
    }
}

/// The filter, sort order and view of a folder list: the saved preferences, unless the request
/// overrides them
#[derive(Debug, Clone, Copy)]
pub struct BrowserOptions {
    pub filter: PhotoFilter,
    pub sort: FolderSort,
    pub view: FolderView,
}

#[derive(Debug, Default, Deserialize)]
pub struct BrowserParams {
    filter: Option<String>,
    sort: Option<String>,
    view: Option<String>,
}

impl BrowserParams {
    /// Unknown values are ignored
    pub fn resolve(&self, prefs: &Prefs) -> BrowserOptions {
        BrowserOptions {
            filter: self
                .filter
                .as_deref()
                .and_then(|v| v.parse().ok())
                .unwrap_or(prefs.filter),
            sort: self
                .sort
                .as_deref()
                .and_then(|v| v.parse().ok())
                .unwrap_or(prefs.folder_sort),
            view: self
                .view
                .as_deref()
                .and_then(|v| v.parse().ok())
                .unwrap_or(prefs.folder_view),
        }
    }
}

fn matches_filter(card: &FolderCard, filter: PhotoFilter) -> bool {
    match filter {
        PhotoFilter::All => true,
        PhotoFilter::Personal => card.folder_type == FolderType::Personal,
        PhotoFilter::Family => card.folder_type == FolderType::Family,
        PhotoFilter::Shared => card.folder_type == FolderType::Shared,
    }
}

/// Applies the filter and the sort order. `uploadable_only` limits the list to the folders the
/// user may upload to (the destination picker).
pub fn filter_and_sort(
    cards: Vec<FolderCard>,
    filter: PhotoFilter,
    sort: FolderSort,
    uploadable_only: bool,
) -> Vec<FolderCard> {
    let mut cards: Vec<FolderCard> = cards
        .into_iter()
        .filter(|card| matches_filter(card, filter) && (!uploadable_only || card.can_upload))
        .collect();
    sort_folders(&mut cards, sort);
    cards
}

/// Ties fall back to the name (A to Z, ignoring case)
fn sort_folders(cards: &mut [FolderCard], sort: FolderSort) {
    use std::cmp::Ordering;

    cards.sort_by_cached_key(|card| (card.name.to_lowercase(), card.id));
    // The sorts below are stable, so the name order is kept for equal elements
    match sort {
        FolderSort::NameAsc => {}
        FolderSort::NameDesc => cards.reverse(),
        FolderSort::CountDesc => cards.sort_by_key(|card| std::cmp::Reverse(card.photo_count)),
        FolderSort::DateDesc => {
            cards.sort_by(|a, b| match (a.latest_photo_at, b.latest_photo_at) {
                (Some(a), Some(b)) => b.cmp(&a),
                // Empty folders last
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            })
        }
    }
}

#[derive(Template)]
#[template(path = "folders/folders_page.html")]
struct FoldersPageTemplate {
    ctx: PageContext,
    browser: FolderBrowser,
}

#[derive(Debug, Deserialize)]
pub struct FoldersQuery {
    q: Option<String>,
}

pub async fn folders_page(
    ctx: PageContext,
    State(state): State<AppStateRef>,
    Query(params): Query<BrowserParams>,
    Query(query): Query<FoldersQuery>,
) -> HttpResult<Response> {
    let options = params.resolve(&ctx.prefs);
    let cards = state.read_pool.get_folder_cards(&ctx.user_id).await?;
    let browser = FolderBrowser::new(cards, options, query.q.unwrap_or_default(), false);

    FoldersPageTemplate {
        ctx: ctx.nav(Nav::Folders),
        browser,
    }
    .try_into_response()
}

#[derive(Template)]
#[template(path = "folders/list.html")]
struct FolderListTemplate {
    browser: FolderBrowser,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    picker: bool,
}

/// `GET /ui/folders/list`: the folder cards, for the Folders tab and the destination picker
pub async fn list_fragment(
    AuthenticatedUser(user): AuthenticatedUser,
    prefs: Prefs,
    State(state): State<AppStateRef>,
    Query(params): Query<BrowserParams>,
    Query(query): Query<ListQuery>,
) -> HttpResult<Response> {
    let options = params.resolve(&prefs);
    let cards = state.read_pool.get_folder_cards(&user.id).await?;

    FolderListTemplate {
        browser: FolderBrowser::new(cards, options, String::new(), query.picker),
    }
    .try_into_response()
}

#[derive(Template)]
#[template(path = "folders/folder_page.html")]
struct FolderPageTemplate {
    ctx: PageContext,
    folder: FolderPageView,
    grid: GridView,
}

/// What the folder page shows and which actions it offers
#[derive(Debug, Clone)]
struct FolderPageView {
    id: i64,
    name: String,
    is_public: bool,
    photo_count: i64,
    /// The `PhotoListContext` key of the photo grid
    context_key: String,
    badge_label: String,
    badge_icon: &'static str,
    /// Upload to the folder
    can_upload: bool,
    /// The empty state's Upload button, empty without `can_upload`
    empty_upload_href: String,
    /// Rename it and change its visibility (owner, or anyone for public folders)
    can_edit: bool,
    /// Manage who it is shared with (private folders of the user)
    can_share: bool,
    /// Look at who it is shared with (folders shared with the user)
    can_view_access: bool,
    /// Delete the (empty) folder; the server also refuses it while trashed photos remain
    can_delete: bool,
    /// Delete photos in the folder
    can_delete_photos: bool,
}

impl FolderPageView {
    fn new(card: &FolderCard) -> Self {
        let (badge_label, badge_icon) = match card.folder_type {
            FolderType::Family => ("Family".to_string(), "family_restroom"),
            FolderType::Personal if card.share_count > 0 => {
                (format!("Shared ({})", card.share_count), "group")
            }
            FolderType::Personal => ("Personal".to_string(), "person"),
            FolderType::Shared => (
                match &card.owner_name {
                    Some(owner) => format!("Shared by {owner}"),
                    None => "Shared".to_string(),
                },
                "group",
            ),
        };
        let can_edit = card.folder_type != FolderType::Shared;

        Self {
            id: card.id,
            name: card.name.clone(),
            is_public: card.owner_id.is_none(),
            photo_count: card.photo_count,
            context_key: PhotoListContext::Folder(card.id).key(),
            badge_label,
            badge_icon,
            can_upload: card.can_upload,
            empty_upload_href: if card.can_upload {
                format!("/upload?folder_id={}", card.id)
            } else {
                String::new()
            },
            can_edit,
            can_share: card.folder_type == FolderType::Personal,
            can_view_access: card.folder_type == FolderType::Shared,
            can_delete: can_edit && card.photo_count == 0,
            can_delete_photos: card.can_delete,
        }
    }
}

pub async fn folder_page(
    ctx: PageContext,
    State(state): State<AppStateRef>,
    Path(folder_id): Path<String>,
) -> HttpResult<Response> {
    let ctx = ctx.nav(Nav::Folders);
    let Ok(folder_id) = folder_id.parse::<i64>() else {
        return Ok(not_found_page(ctx));
    };
    let Some(card) = state
        .read_pool
        .get_folder_card(&ctx.user_id, folder_id)
        .await?
    else {
        return Ok(not_found_page(ctx));
    };
    let grid = build_grid_view(
        state,
        &ctx.user_id,
        &PhotoListContext::Folder(folder_id),
        &ctx.prefs,
    )
    .await?;

    FolderPageTemplate {
        ctx,
        folder: FolderPageView::new(&card),
        grid,
    }
    .try_into_response()
}

#[derive(Template)]
#[template(path = "folders/name_dialog.html")]
struct FolderNameDialogTemplate {
    /// Renaming an existing folder instead of creating one
    renaming: bool,
    name: String,
    is_public: bool,
}

#[derive(Debug, Deserialize)]
pub struct FolderNameQuery {
    /// Rename this folder; its current name and visibility are used
    folder_id: Option<i64>,
    /// Initial values when creating
    name: Option<String>,
    #[serde(default)]
    public: bool,
}

/// `GET /ui/dialogs/folder-name`: the form to create or rename a folder
pub async fn folder_name_dialog(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppStateRef>,
    Query(query): Query<FolderNameQuery>,
) -> HttpResult<Response> {
    let Some(folder_id) = query.folder_id else {
        return FolderNameDialogTemplate {
            renaming: false,
            name: query.name.unwrap_or_default(),
            is_public: query.public,
        }
        .try_into_response();
    };

    // Same rule as `PATCH /api/folders/{id}`
    let card = state
        .read_pool
        .get_folder_card(&user.id, folder_id)
        .await?
        .filter(|card| card.folder_type != FolderType::Shared)
        .ok_or(HttpError::NotFound)?;

    FolderNameDialogTemplate {
        renaming: true,
        name: card.name,
        is_public: card.owner_id.is_none(),
    }
    .try_into_response()
}

/// Old bookmarks used `/folder/{id}`
pub async fn legacy_folder_redirect(
    auth: AuthSession,
    prefs: Prefs,
    Path(folder_id): Path<String>,
) -> Response {
    match folder_id.parse::<i64>() {
        Ok(id) => Redirect::permanent(&format!("/folders/{id}")).into_response(),
        Err(_) => not_found(auth, prefs).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn card(
        id: i64,
        name: &str,
        folder_type: FolderType,
        count: i64,
        latest: Option<i64>,
    ) -> FolderCard {
        FolderCard {
            id,
            name: name.to_string(),
            owner_id: (folder_type != FolderType::Family).then(|| "someone".to_string()),
            owner_name: Some("Someone".to_string()),
            folder_type,
            photo_count: count,
            cover_photo_id: latest.map(|_| id * 10),
            cover_thumb_hash: None,
            latest_photo_at: latest
                .map(|day| datetime!(2024-01-01 00:00 UTC) + time::Duration::days(day)),
            can_upload: folder_type != FolderType::Shared,
            can_delete: true,
            share_count: 0,
        }
    }

    fn cards() -> Vec<FolderCard> {
        vec![
            card(1, "beach", FolderType::Personal, 5, Some(10)),
            card(2, "Alps", FolderType::Family, 5, Some(30)),
            card(3, "empty", FolderType::Personal, 0, None),
            card(4, "Zoo", FolderType::Shared, 9, Some(20)),
            card(5, "Beach", FolderType::Family, 1, Some(10)),
        ]
    }

    fn ids(cards: Vec<FolderCard>) -> Vec<i64> {
        cards.iter().map(|card| card.id).collect()
    }

    #[test]
    fn test_sort_orders() {
        let sorted = |sort| ids(filter_and_sort(cards(), PhotoFilter::All, sort, false));

        // Names ignore case; equal names keep the id order
        assert_eq!(sorted(FolderSort::NameAsc), [2, 1, 5, 3, 4]);
        assert_eq!(sorted(FolderSort::NameDesc), [4, 3, 5, 1, 2]);
        // Ties (5 photos) fall back to the name, A to Z
        assert_eq!(sorted(FolderSort::CountDesc), [4, 2, 1, 5, 3]);
        // Newest first, empty folders last; the tie of the two 10-day folders goes by name
        assert_eq!(sorted(FolderSort::DateDesc), [2, 4, 1, 5, 3]);
    }

    #[test]
    fn test_filters_follow_the_folder_type() {
        let filtered = |filter| ids(filter_and_sort(cards(), filter, FolderSort::NameAsc, false));

        assert_eq!(filtered(PhotoFilter::All).len(), 5);
        assert_eq!(filtered(PhotoFilter::Personal), [1, 3]);
        assert_eq!(filtered(PhotoFilter::Family), [2, 5]);
        assert_eq!(filtered(PhotoFilter::Shared), [4]);
    }

    #[test]
    fn test_picker_only_lists_uploadable_folders() {
        let uploadable = filter_and_sort(cards(), PhotoFilter::All, FolderSort::NameAsc, true);
        assert_eq!(ids(uploadable), [2, 1, 5, 3]);
    }

    #[test]
    fn test_card_view_labels() {
        let view = FolderCardView::new(&card(4, "Zoo", FolderType::Shared, 1, Some(1)));
        assert_eq!(view.count_label, "1 photo");
        assert_eq!(view.list_details, "1 photo • Shared • Someone");
        assert_eq!(view.type_key, "shared");
        assert!(!view.is_public);

        let view = FolderCardView::new(&card(2, "Alps", FolderType::Family, 0, None));
        assert_eq!(view.list_details, "0 photos • Family");
        assert!(view.is_public);
        assert_eq!(view.cover_photo_id, None);
    }

    #[test]
    fn test_browser_params_fall_back_to_prefs() {
        let prefs = Prefs {
            filter: PhotoFilter::Family,
            folder_sort: FolderSort::CountDesc,
            folder_view: FolderView::List,
            ..Prefs::default()
        };
        let params = BrowserParams {
            filter: Some("shared".into()),
            sort: Some("nonsense".into()),
            view: None,
        };
        let options = params.resolve(&prefs);
        assert_eq!(options.filter, PhotoFilter::Shared);
        assert_eq!(options.sort, FolderSort::CountDesc);
        assert_eq!(options.view, FolderView::List);
    }
}
