use crate::http::AppState;
use crate::http::auth::AuthenticatedUser;
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
    #[expect(dead_code, reason = "Read by the pages of later packages")]
    pub asset_version: &'static str,
    pub nav_active: Nav,
    pub prefs: Prefs,
}

impl PageContext {
    pub fn new(user: &User, prefs: Prefs) -> Self {
        Self {
            user_id: user.id.clone(),
            user_display_name: user.name.clone(),
            asset_version: AppState::ASSET_VERSION,
            nav_active: Nav::None,
            prefs,
        }
    }

    pub fn nav(mut self, nav: Nav) -> Self {
        self.nav_active = nav;
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
        Ok(Self::new(&user, prefs))
    }
}
