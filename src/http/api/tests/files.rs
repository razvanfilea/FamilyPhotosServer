use super::TestServer;
use crate::repo::tests::{insert_photo, photo};
use axum::http::{Method, StatusCode, header};
use serde_json::json;
use sqlx::SqlitePool;

#[sqlx::test]
async fn test_download_caching(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;
    let photo = insert_photo(server.pool(), photo(Some("alice"), "Plajă.jpg")).await;
    server.write_file(&photo, None, b"0123456789");
    let uri = format!("/api/download/{}", photo.id);

    let response = alice.get(&uri).await.assert_status(StatusCode::OK);
    assert_eq!(response.body, b"0123456789");
    assert_eq!(response.header(header::CONTENT_TYPE), "image/jpeg");
    assert_eq!(
        response.header(header::CACHE_CONTROL),
        "private, max-age=31536000, immutable"
    );
    assert_eq!(
        response.header(header::CONTENT_DISPOSITION),
        "attachment; filename=\"Plaj_.jpg\"; filename*=UTF-8''Plaj%C4%83.jpg"
    );
    let etag = response.header(header::ETAG);
    assert!(etag.starts_with("W/\""), "{etag}");
    let last_modified = response.header(header::LAST_MODIFIED);

    let inline = alice.get(&format!("{uri}?inline=1")).await;
    assert!(
        inline
            .header(header::CONTENT_DISPOSITION)
            .starts_with("inline;")
    );

    // Revalidation
    let not_modified = alice.get_with(&uri, &[(header::IF_NONE_MATCH, etag)]).await;
    assert_eq!(not_modified.status, StatusCode::NOT_MODIFIED);
    assert!(not_modified.body.is_empty());
    assert_eq!(not_modified.header(header::ETAG), etag);
    for (name, value, status) in [
        (
            header::IF_MODIFIED_SINCE,
            last_modified,
            StatusCode::NOT_MODIFIED,
        ),
        (header::IF_NONE_MATCH, "W/\"other\"", StatusCode::OK),
    ] {
        let response = alice.get_with(&uri, &[(name.clone(), value)]).await;
        assert_eq!(response.status, status, "{name}: {value}");
    }

    // Ranges
    let partial = alice.get_with(&uri, &[(header::RANGE, "bytes=2-5")]).await;
    assert_eq!(partial.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(partial.body, b"2345");
    assert_eq!(partial.header(header::CONTENT_RANGE), "bytes 2-5/10");
    assert_eq!(partial.header(header::CONTENT_LENGTH), "4");
    // The ETag is weak, which If-Range can't match: only the date resumes a download
    for (if_range, status, len) in [
        (last_modified, StatusCode::PARTIAL_CONTENT, 4),
        (etag, StatusCode::OK, 10),
        ("W/\"old\"", StatusCode::OK, 10),
    ] {
        let response = alice
            .get_with(
                &uri,
                &[(header::RANGE, "bytes=2-5"), (header::IF_RANGE, if_range)],
            )
            .await;
        assert_eq!(
            (response.status, response.body.len()),
            (status, len),
            "{if_range}"
        );
    }
    let unsatisfiable = alice.get_with(&uri, &[(header::RANGE, "bytes=50-")]).await;
    assert_eq!(unsatisfiable.status, StatusCode::RANGE_NOT_SATISFIABLE);

    let head = alice.request(Method::HEAD, &uri, &[], None).await;
    assert_eq!(head.status, StatusCode::OK);
    assert_eq!(head.header(header::CONTENT_LENGTH), "10");
    assert!(head.body.is_empty());
}

#[sqlx::test]
async fn test_preview_and_screen(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;
    let broken = insert_photo(server.pool(), photo(Some("alice"), "broken.jpg")).await;
    server.write_file(&broken, None, b"not an image");
    let preview_uri = format!("/api/preview/{}", broken.id);

    // The screen rendition (web only) is unavailable
    let response = alice.get(&format!("/api/screen/{}", broken.id)).await;
    assert_eq!(response.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.header(header::RETRY_AFTER), "5");
    assert_eq!(response.header(header::CACHE_CONTROL), "no-store");

    // The preview falls back to the original (owner decision, for Android), uncacheable
    let response = alice.get(&preview_uri).await.assert_status(StatusCode::OK);
    assert_eq!(response.body, b"not an image");
    assert_eq!(response.header(header::CONTENT_TYPE), "image/jpeg");
    assert_eq!(response.header(header::CACHE_CONTROL), "no-store");
    assert!(response.headers.get(header::ETAG).is_none());
    assert!(response.headers.get(header::LAST_MODIFIED).is_none());

    // Conditional requests can't pin the fallback; ranges still work
    let headers = [(header::IF_NONE_MATCH, "*"), (header::RANGE, "bytes=0-2")];
    let response = alice.get_with(&preview_uri, &headers).await;
    assert_eq!(response.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.body, b"not");

    // An existing preview is served without generating anything
    let preview_path = server
        .state
        .storage
        .resolve_preview(broken.partial_preview_path());
    std::fs::write(&preview_path, [7u8; 200]).unwrap();
    let response = alice.get(&preview_uri).await.assert_status(StatusCode::OK);
    assert_eq!(response.body, [7u8; 200]);
    assert_eq!(
        response.header(header::CACHE_CONTROL),
        "private, max-age=604800"
    );
    assert!(response.header(header::ETAG).starts_with("W/"));
    assert!(
        response
            .header(header::CONTENT_DISPOSITION)
            .starts_with("inline;")
    );
    let etag = response.header(header::ETAG);
    let response = alice
        .get_with(&preview_uri, &[(header::IF_NONE_MATCH, etag)])
        .await;
    assert_eq!(response.status, StatusCode::NOT_MODIFIED);

    // Videos have no screen rendition, GIFs are served as they are
    let video = insert_photo(server.pool(), photo(Some("alice"), "clip.mp4")).await;
    let response = alice.get(&format!("/api/screen/{}", video.id)).await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    let gif = insert_photo(server.pool(), photo(Some("alice"), "funny.gif")).await;
    server.write_file(&gif, None, b"GIF89a...");
    let response = alice.get(&format!("/api/screen/{}", gif.id)).await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.body, b"GIF89a...");
    assert_eq!(response.header(header::CONTENT_TYPE), "image/gif");

    // Deleting a photo permanently removes its renditions. Only trashed photos can be deleted.
    let screen_path = server
        .state
        .storage
        .resolve_preview(format!("screen/{}.webp", broken.id));
    std::fs::create_dir_all(screen_path.parent().unwrap()).unwrap();
    std::fs::write(&screen_path, [7u8; 200]).unwrap();
    let delete_uri = format!("/api/delete/{}", broken.id);
    let response = alice.request(Method::DELETE, &delete_uri, &[], None).await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    let response = alice.post("/api/trash", json!([broken.id])).await;
    assert_eq!(response.status, StatusCode::OK);
    let response = alice.request(Method::DELETE, &delete_uri, &[], None).await;
    assert_eq!(response.status, StatusCode::OK);
    assert!(!preview_path.exists());
    assert!(!screen_path.exists());
}
