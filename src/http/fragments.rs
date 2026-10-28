//! Helpers for the HTML fragment routes under `/ui/*`, which are only requested by HTMX/JS.

use crate::http::utils::AuthSession;
use axum::extract::Request;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// Middleware for fragment routes: when logged out, respond with 401 and an `HX-Redirect` to the
/// login page (returning to the page the fragment was requested from), instead of the page
/// router's 302 which HTMX would follow and swap the login page into the fragment target.
pub async fn require_login(auth: AuthSession, request: Request, next: Next) -> Response {
    if auth.user().await.is_some() {
        return next.run(request).await;
    }

    let current_path = request
        .headers()
        .get("hx-current-url")
        .and_then(|value| value.to_str().ok())
        .and_then(path_of_url);

    let mut headers = HeaderMap::new();
    if let Ok(location) = HeaderValue::from_str(&login_url(current_path.as_deref())) {
        headers.insert("hx-redirect", location);
    }

    (StatusCode::UNAUTHORIZED, headers).into_response()
}

/// The login page URL, returning to `next` after logging in if it's a safe path
pub fn login_url(next: Option<&str>) -> String {
    match next.and_then(safe_redirect_path) {
        Some(next) if next != "/" => {
            let encoded: String = form_urlencoded::byte_serialize(next.as_bytes()).collect();
            format!("/login?next={encoded}")
        }
        _ => "/login".to_string(),
    }
}

/// Returns `path` if it's a same-origin relative path (like `/folders?q=a`), to prevent open
/// redirects through `?next=`.
pub fn safe_redirect_path(path: &str) -> Option<&str> {
    let is_safe = path.starts_with('/')
        && !path.starts_with("//")
        && !path.starts_with("/\\")
        && !path.chars().any(|c| c.is_control() || c == '\\');

    is_safe.then_some(path)
}

/// Extracts the path and query from an absolute URL (`https://host/a?b` → `/a?b`)
fn path_of_url(url: &str) -> Option<String> {
    let (_, rest) = url.split_once("://")?;
    let path = rest.find('/').map_or("/", |start| &rest[start..]);
    let path = path.split('#').next().unwrap_or(path);
    safe_redirect_path(path).map(ToOwned::to_owned)
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Foundation for the fragment routes of later packages"
    )
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Error,
    Info,
}

impl ToastKind {
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "Foundation for the fragment routes of later packages"
        )
    )]
    fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Error => "error",
            Self::Info => "info",
        }
    }
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Foundation for the fragment routes of later packages"
    )
)]
/// Headers that make `assets/js/core/toast.js` show a toast when HTMX receives the response
pub fn toast_trigger(message: &str, kind: ToastKind) -> HeaderMap {
    let payload = json!({ "fp:toast": { "message": message, "type": kind.as_str() } }).to_string();

    let mut headers = HeaderMap::new();
    let value = HeaderValue::from_str(&escape_non_ascii(&payload))
        .expect("Escaped JSON is always a valid header value");
    headers.insert("hx-trigger", value);
    headers
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Foundation for the fragment routes of later packages"
    )
)]
/// Header values must be visible ASCII, so escape everything else as JSON `\uXXXX` sequences
fn escape_non_ascii(json: &str) -> String {
    let mut escaped = String::with_capacity(json.len());
    for c in json.chars() {
        if c.is_ascii() && !c.is_ascii_control() {
            escaped.push(c);
        } else {
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                escaped.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_redirect_path() {
        assert_eq!(safe_redirect_path("/"), Some("/"));
        assert_eq!(
            safe_redirect_path("/folders/3?photo=1"),
            Some("/folders/3?photo=1")
        );
        assert_eq!(safe_redirect_path("//evil.com"), None);
        assert_eq!(safe_redirect_path("/\\evil.com"), None);
        assert_eq!(safe_redirect_path("https://evil.com"), None);
        assert_eq!(safe_redirect_path("folders"), None);
        assert_eq!(safe_redirect_path("/a\nb"), None);
        assert_eq!(safe_redirect_path(""), None);
    }

    #[test]
    fn test_login_url() {
        assert_eq!(login_url(None), "/login");
        assert_eq!(login_url(Some("/")), "/login");
        assert_eq!(login_url(Some("//evil.com")), "/login");
        assert_eq!(
            login_url(Some("/folders?q=a b&x=1")),
            "/login?next=%2Ffolders%3Fq%3Da+b%26x%3D1"
        );
    }

    #[test]
    fn test_path_of_url() {
        assert_eq!(
            path_of_url("http://localhost:8080/folders/2?photo=5#x").as_deref(),
            Some("/folders/2?photo=5")
        );
        assert_eq!(path_of_url("https://host").as_deref(), Some("/"));
        assert_eq!(path_of_url("/relative"), None);
    }

    #[test]
    fn test_toast_trigger() {
        let headers = toast_trigger("Moved to „Vacanță”", ToastKind::Success);
        let value = headers.get("hx-trigger").unwrap().to_str().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(value).unwrap();
        assert_eq!(parsed["fp:toast"]["message"], "Moved to „Vacanță”");
        assert_eq!(parsed["fp:toast"]["type"], "success");

        for (kind, name) in [(ToastKind::Error, "error"), (ToastKind::Info, "info")] {
            let headers = toast_trigger("x", kind);
            let value = headers.get("hx-trigger").unwrap().to_str().unwrap();
            assert!(value.contains(&format!(r#""type":"{name}""#)));
        }
    }
}
