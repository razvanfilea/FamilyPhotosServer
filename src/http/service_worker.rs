//! `/sw.js`: the service worker script. It is served from the root (not from `/assets`) so its scope
//! is the whole site, with the asset version and the files to precache filled in: a new version of the
//! assets makes a new worker.

use std::path::Path;

use axum::http::{HeaderValue, header};
use axum::response::IntoResponse;
use tokio::task::spawn_blocking;
use walkdir::WalkDir;

use super::AppState;
use super::error::HttpError;

const SCRIPT: &str = include_str!("../../assets/sw.js");
const ASSETS_DIR: &str = "assets";
const OFFLINE_PAGE: &str = "/assets/offline.html";
/// Asset directories the pages need to run. Not `fav`: the browser fetches those icons itself
const PRECACHED_DIRS: [&str; 4] = ["css", "js", "vendor", "icons"];

pub async fn script() -> Result<impl IntoResponse, HttpError> {
    let paths = spawn_blocking(|| precache_paths(Path::new(ASSETS_DIR), AppState::ASSET_VERSION))
        .await
        .map_err(|e| HttpError::Internal(e.to_string()))?;
    let body = render(AppState::ASSET_VERSION, &paths);

    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/javascript; charset=utf-8"),
            ),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
        ],
        body,
    ))
}

fn render(version: &str, paths: &[String]) -> String {
    let paths = serde_json::to_string(paths).expect("a list of strings is valid JSON");
    SCRIPT
        .replace("__ASSET_VERSION__", version)
        .replace("__PRECACHE_PATHS__", &paths)
}

/// The URL paths the worker stores when it installs. A development server serves assets that change
/// without a new version, so there only the offline page is stored.
fn precache_paths(assets: &Path, version: &str) -> Vec<String> {
    let mut paths = vec![OFFLINE_PAGE.to_string()];
    if version == "dev" {
        return paths;
    }
    let mut files: Vec<String> = PRECACHED_DIRS
        .iter()
        .flat_map(|dir| WalkDir::new(assets.join(dir)).into_iter().flatten())
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| {
            entry
                .path()
                .strip_prefix(assets)
                .ok()
                .map(|relative| format!("/assets/{}", relative.to_string_lossy()))
        })
        .collect();
    files.sort();
    paths.extend(files);
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(files: &[&str]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for file in files {
            let path = root.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "x").unwrap();
        }
        root
    }

    #[test]
    fn test_precache_paths() {
        let root = tree(&[
            "css/main.css",
            "js/app.js",
            "js/core/api.js",
            "icons/sprite.svg",
            "fav/icon-512.png",
            "manifest.webmanifest",
            "sw.js",
        ]);
        let cases = [
            (
                "abc",
                vec![
                    "/assets/offline.html",
                    "/assets/css/main.css",
                    "/assets/icons/sprite.svg",
                    "/assets/js/app.js",
                    "/assets/js/core/api.js",
                ],
            ),
            ("dev", vec!["/assets/offline.html"]),
        ];
        for (version, expected) in cases {
            assert_eq!(
                precache_paths(root.path(), version),
                expected,
                "version {version}"
            );
        }
    }

    #[test]
    fn test_render_fills_both_placeholders() {
        let rendered = render("abc", &["/assets/js/app.js".to_string()]);
        assert!(rendered.contains("const VERSION = 'abc';"));
        assert!(rendered.contains(r#"const PRECACHE = ["/assets/js/app.js"];"#));
        assert!(
            !rendered.contains("__"),
            "a placeholder is left in the script"
        );
    }
}
