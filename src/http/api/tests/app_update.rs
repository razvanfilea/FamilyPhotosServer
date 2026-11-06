//! App updates: `X-App-Latest-Version` on the photo sync routes and the public APK download.

use super::TestServer;
use crate::http::api::sync::APP_LATEST_VERSION_HEADER;
use axum::http::{Method, StatusCode, header};
use serde_json::json;
use sqlx::SqlitePool;

/// The log is empty, so 0 is up to date: 200
const PARTIAL: &str = "/api/sync/partial?last_synced_event_id=0";
/// A cursor ahead of the log: 409
const PARTIAL_STALE: &str = "/api/sync/partial?last_synced_event_id=999999";
const FULL: &str = "/api/sync/full";

#[sqlx::test]
async fn test_latest_version_header_on_photo_sync(pool: SqlitePool) {
    let server = TestServer::with_app_release(pool, 303, Some(b"apk")).await;
    let alice = server.login("alice").await;

    for (uri, status) in [
        (PARTIAL, StatusCode::OK),
        (PARTIAL_STALE, StatusCode::CONFLICT),
        (FULL, StatusCode::OK),
    ] {
        let response = alice.get(uri).await.assert_status(status);
        assert_eq!(response.header(APP_LATEST_VERSION_HEADER), "303", "{uri}");
    }

    // The bodies are unchanged
    assert_eq!(
        alice.get(FULL).await.json(),
        json!({"event_log_id": 0, "photos": []})
    );
    assert_eq!(
        alice.get(PARTIAL).await.json(),
        json!({"event_log_id": 0, "events": []})
    );

    // Other routes don't announce it
    let folders = alice
        .post("/api/sync/folders", json!({}))
        .await
        .assert_status(StatusCode::OK);
    assert!(!folders.headers.contains_key(APP_LATEST_VERSION_HEADER));
    let profile = alice.get("/api/profile").await;
    assert!(!profile.headers.contains_key(APP_LATEST_VERSION_HEADER));

    // Logged out: 401, and nothing announced
    let anonymous = server.anonymous().get(FULL).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    assert!(!anonymous.headers.contains_key(APP_LATEST_VERSION_HEADER));
}

async fn assert_no_latest_version_header(server: &TestServer) {
    let alice = server.login("alice").await;
    for (uri, status) in [
        (PARTIAL, StatusCode::OK),
        (PARTIAL_STALE, StatusCode::CONFLICT),
        (FULL, StatusCode::OK),
    ] {
        let response = alice.get(uri).await.assert_status(status);
        assert!(
            !response.headers.contains_key(APP_LATEST_VERSION_HEADER),
            "{uri}"
        );
    }
}

#[sqlx::test]
async fn test_no_latest_version_header_when_not_configured(pool: SqlitePool) {
    assert_no_latest_version_header(&TestServer::new(pool).await).await;
}

/// Users must never be told to update to an APK they can't download
#[sqlx::test]
async fn test_no_latest_version_header_when_apk_missing(pool: SqlitePool) {
    assert_no_latest_version_header(&TestServer::with_app_release(pool, 303, None).await).await;
}

const DOWNLOAD: &str = "/api/app/download";
const APK: &[u8] = b"PK\x03\x04 not really an APK";

#[sqlx::test]
async fn test_download_apk(pool: SqlitePool) {
    let server = TestServer::with_app_release(pool, 303, Some(APK)).await;

    // Public, and the app's authenticated requests work too
    let alice = server.login("alice").await;
    for client in [server.anonymous(), alice] {
        let response = client.get(DOWNLOAD).await.assert_status(StatusCode::OK);
        assert_eq!(response.body, APK);
        assert_eq!(
            response.header(header::CONTENT_TYPE),
            "application/vnd.android.package-archive"
        );
        assert_eq!(
            response.header(header::CONTENT_DISPOSITION),
            "attachment; filename=\"FamilyPhotos-303.apk\"; filename*=UTF-8''FamilyPhotos-303.apk"
        );
        assert_eq!(
            response.header(header::CONTENT_LENGTH),
            APK.len().to_string()
        );
        assert_eq!(response.header(header::CACHE_CONTROL), "no-cache");
        assert!(!response.headers.contains_key(header::CONTENT_ENCODING));
    }

    // Revalidation: unchanged until the APK changes
    let anonymous = server.anonymous();
    let etag = anonymous
        .get(DOWNLOAD)
        .await
        .header(header::ETAG)
        .to_owned();
    anonymous
        .get_with(DOWNLOAD, &[(header::IF_NONE_MATCH, &etag)])
        .await
        .assert_status(StatusCode::NOT_MODIFIED);

    let apk_path = &server.state.app_release.as_ref().unwrap().apk_path;
    const NEWER_APK: &[u8] = b"PK\x03\x04 a newer, bigger APK";
    std::fs::write(apk_path, NEWER_APK).unwrap();
    let response = anonymous
        .get_with(DOWNLOAD, &[(header::IF_NONE_MATCH, &etag)])
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(response.body, NEWER_APK);

    // HEAD has the length without the body
    let head = anonymous
        .request(Method::HEAD, DOWNLOAD, &[], None)
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(
        head.header(header::CONTENT_LENGTH),
        NEWER_APK.len().to_string()
    );
    assert!(head.body.is_empty());
}

#[sqlx::test]
async fn test_download_apk_not_configured(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    server
        .anonymous()
        .get(DOWNLOAD)
        .await
        .assert_status(StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn test_download_apk_missing(pool: SqlitePool) {
    let server = TestServer::with_app_release(pool, 303, None).await;
    let alice = server.login("alice").await;
    for client in [server.anonymous(), alice] {
        client
            .get(DOWNLOAD)
            .await
            .assert_status(StatusCode::NOT_FOUND);
    }
}
