use crate::model::photo_filter::PhotoFilter;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{HeaderMap, header};
use std::convert::Infallible;
use std::str::FromStr;

/// Name of the cookie holding the user preferences, shared with `assets/js/core/prefs.js`
pub const PREFS_COOKIE: &str = "fp_prefs";

pub const MAX_ZOOM: u8 = 3;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FolderSort {
    DateDesc,
    #[default]
    NameAsc,
    NameDesc,
    CountDesc,
}

impl FolderSort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DateDesc => "date_desc",
            Self::NameAsc => "name_asc",
            Self::NameDesc => "name_desc",
            Self::CountDesc => "count_desc",
        }
    }
}

impl FromStr for FolderSort {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "date_desc" => Ok(Self::DateDesc),
            "name_asc" => Ok(Self::NameAsc),
            "name_desc" => Ok(Self::NameDesc),
            "count_desc" => Ok(Self::CountDesc),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FolderView {
    #[default]
    Grid,
    List,
}

impl FolderView {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Grid => "grid",
            Self::List => "list",
        }
    }
}

impl FromStr for FolderView {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "grid" => Ok(Self::Grid),
            "list" => Ok(Self::List),
            _ => Err(()),
        }
    }
}

/// User preferences persisted client-side in the `fp_prefs` cookie, so pages render the right
/// state on first paint. Missing or invalid values fall back to their defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prefs {
    pub filter: PhotoFilter,
    /// Grid zoom level, `0..=MAX_ZOOM`
    pub zoom: u8,
    pub folder_sort: FolderSort,
    pub folder_view: FolderView,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            filter: PhotoFilter::default(),
            zoom: 1,
            folder_sort: FolderSort::default(),
            folder_view: FolderView::default(),
        }
    }
}

impl Prefs {
    /// Parses the cookie value: a URL-encoded form string like `filter=all&zoom=1`
    pub fn parse(value: &str) -> Self {
        let mut prefs = Self::default();

        for (key, value) in form_urlencoded::parse(value.as_bytes()) {
            match key.as_ref() {
                "filter" => {
                    if let Ok(filter) = value.parse() {
                        prefs.filter = filter;
                    }
                }
                "zoom" => {
                    if let Ok(zoom) = value.parse::<u8>()
                        && zoom <= MAX_ZOOM
                    {
                        prefs.zoom = zoom;
                    }
                }
                "fsort" => {
                    if let Ok(sort) = value.parse() {
                        prefs.folder_sort = sort;
                    }
                }
                "fview" => {
                    if let Ok(view) = value.parse() {
                        prefs.folder_view = view;
                    }
                }
                _ => {}
            }
        }

        prefs
    }

    pub fn from_headers(headers: &HeaderMap) -> Self {
        headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|cookies| cookies.split(';'))
            .find_map(|cookie| {
                let (name, value) = cookie.trim().split_once('=')?;
                (name == PREFS_COOKIE).then_some(value)
            })
            .map(Self::parse)
            .unwrap_or_default()
    }
}

impl<S> FromRequestParts<S> for Prefs
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self::from_headers(&parts.headers))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn test_parse_all_values() {
        let prefs = Prefs::parse("filter=shared&zoom=3&fsort=count_desc&fview=list");
        assert_eq!(
            prefs,
            Prefs {
                filter: PhotoFilter::Shared,
                zoom: 3,
                folder_sort: FolderSort::CountDesc,
                folder_view: FolderView::List,
            }
        );
    }

    #[test]
    fn test_parse_invalid_values_fall_back_to_defaults() {
        assert_eq!(Prefs::parse(""), Prefs::default());
        assert_eq!(
            Prefs::parse("filter=everything&zoom=4&fsort=size&fview=&unknown=1"),
            Prefs::default()
        );
        assert_eq!(Prefs::parse("zoom=-1").zoom, 1);
        assert_eq!(Prefs::parse("zoom=0").zoom, 0);
        assert_eq!(Prefs::parse("%%%&&==").filter, PhotoFilter::All);
    }

    #[test]
    fn test_defaults() {
        let prefs = Prefs::default();
        assert_eq!(prefs.filter, PhotoFilter::All);
        assert_eq!(prefs.zoom, 1);
        assert_eq!(prefs.folder_sort, FolderSort::NameAsc);
        assert_eq!(prefs.folder_view, FolderView::Grid);
    }

    #[test]
    fn test_from_headers() {
        let mut headers = HeaderMap::new();
        assert_eq!(Prefs::from_headers(&headers), Prefs::default());

        headers.append(
            header::COOKIE,
            HeaderValue::from_static("id=abc; fp_prefs=filter=family&zoom=2; other=x"),
        );
        let prefs = Prefs::from_headers(&headers);
        assert_eq!(prefs.filter, PhotoFilter::Family);
        assert_eq!(prefs.zoom, 2);
        assert_eq!(prefs.filter.as_str(), "family");
    }

    #[test]
    fn test_enum_round_trip() {
        for sort in [
            FolderSort::DateDesc,
            FolderSort::NameAsc,
            FolderSort::NameDesc,
            FolderSort::CountDesc,
        ] {
            assert_eq!(sort.as_str().parse::<FolderSort>(), Ok(sort));
        }
        for view in [FolderView::Grid, FolderView::List] {
            assert_eq!(view.as_str().parse::<FolderView>(), Ok(view));
        }
    }
}
