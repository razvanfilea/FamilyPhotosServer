use crate::http::AppState;
use crate::http::auth::AuthenticatedUser;
use crate::http::context::PhotoListContext;
use crate::http::error::HttpError;
use crate::http::prefs::Prefs;
use crate::model::user::User;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;

/// The navigation destination a page belongs to, used to highlight the nav items
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Nav {
    #[default]
    None,
    Photos,
    Folders,
    /// The Utilities hub and the tools without their own nav item (duplicates, settings, …)
    Utilities,
    Favorites,
    Trash,
    Upload,
}

impl Nav {
    /// Key of the nav item representing exactly this destination
    pub fn key(&self) -> &'static str {
        match self {
            Self::None => "",
            Self::Photos => "photos",
            Self::Folders => "folders",
            Self::Utilities => "utilities",
            Self::Favorites => "favorites",
            Self::Trash => "trash",
            Self::Upload => "upload",
        }
    }

    /// Key of the primary tab (Photos / Folders / Utilities) this destination lives under
    pub fn tab(&self) -> &'static str {
        match self {
            Self::Photos => "photos",
            Self::Folders => "folders",
            Self::Utilities | Self::Favorites | Self::Trash => "utilities",
            Self::None | Self::Upload => "",
        }
    }
}

/// Data every full page rendered inside `shell.html` needs; templates embed it as `ctx`.
///
/// Extracted from the request (requires a logged-in user); handlers then set the active nav item
/// with [`PageContext::nav`].
#[derive(Debug, Clone)]
pub struct PageContext {
    pub user_id: String,
    pub user_display_name: String,
    pub asset_version: &'static str,
    pub nav_active: Nav,
    pub prefs: Prefs,
    /// What the photo viewer opens on page load, see `templates/viewer/root.html`
    pub viewer: ViewerLink,
}

/// A deep link into the photo viewer: `?photo={id}`, plus `&ctx={contextKey}` when the photo was
/// opened in a context other than the page's own (e.g. a memory on the timeline).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewerLink {
    pub photo_id: Option<i64>,
    /// The viewer's context; the page's own context unless the URL names another one
    pub context_key: Option<String>,
}

impl ViewerLink {
    /// Reads `photo` and `ctx` from a query string, ignoring invalid values
    pub fn from_query(query: &str) -> Self {
        let mut link = Self::default();
        for (key, value) in form_urlencoded::parse(query.as_bytes()) {
            match key.as_ref() {
                "photo" => link.photo_id = value.parse().ok(),
                "ctx" => {
                    link.context_key = value.parse::<PhotoListContext>().ok().map(|ctx| ctx.key())
                }
                _ => {}
            }
        }
        link
    }
}

impl PageContext {
    pub fn new(user: &User, prefs: Prefs) -> Self {
        Self {
            user_id: user.id.clone(),
            user_display_name: user.name.clone(),
            asset_version: AppState::ASSET_VERSION,
            nav_active: Nav::None,
            prefs,
            viewer: ViewerLink::default(),
        }
    }

    pub fn nav(mut self, nav: Nav) -> Self {
        self.nav_active = nav;
        self
    }

    /// Sets the context the viewer walks through when the page is opened with `?photo=`, unless
    /// the URL names one with `&ctx=`. Pages with a photo grid pass the grid's context.
    pub fn viewer_context(mut self, context: &PhotoListContext) -> Self {
        if self.viewer.context_key.is_none() {
            self.viewer.context_key = Some(context.key());
        }
        self
    }
}

impl<S> FromRequestParts<S> for PageContext
where
    S: Send + Sync,
{
    type Rejection = HttpError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let AuthenticatedUser(user) = AuthenticatedUser::from_request_parts(parts, state).await?;
        let prefs = Prefs::from_headers(&parts.headers);
        let mut ctx = Self::new(&user, prefs);
        ctx.viewer = ViewerLink::from_query(parts.uri.query().unwrap_or_default());
        Ok(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewer_link_from_query() {
        let link = |photo_id: Option<i64>, context_key: Option<&str>| ViewerLink {
            photo_id,
            context_key: context_key.map(String::from),
        };
        let cases = [
            ("", link(None, None)),
            ("photo=12", link(Some(12), None)),
            ("filter=family&photo=7", link(Some(7), None)),
            (
                "photo=7&ctx=memories%3A2",
                link(Some(7), Some("memories:2")),
            ),
            (
                "photo=7&ctx=memories:2:all",
                link(Some(7), Some("memories:2")),
            ),
            ("photo=7&ctx=folder:3", link(Some(7), Some("folder:3"))),
            ("photo=abc&ctx=nonsense", link(None, None)),
            ("ctx=memories:11", link(None, None)),
        ];
        for (query, expected) in cases {
            assert_eq!(ViewerLink::from_query(query), expected, "{query}");
        }
    }
}
