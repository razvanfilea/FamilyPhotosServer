//! Router-level tests of the page shell: auth redirects, fragments, fallbacks and headers.

use crate::http::api::tests::{PASSWORD, TestServer};
use crate::http::asset_cache_control;
use axum::http::{HeaderName, StatusCode, header};
use sqlx::SqlitePool;

fn hx(name: &'static str) -> HeaderName {
    HeaderName::from_static(name)
}

const HX_REQUEST: (HeaderName, &str) = (HeaderName::from_static("hx-request"), "true");

#[sqlx::test]
async fn test_session_cookie_attributes(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let response = server
        .login_with(&format!("user_id=alice&password={PASSWORD}"), &[])
        .await;
    let cookie = response.header(header::SET_COOKIE);
    // Lax: sent when the installed app is opened or a link is followed from another app
    for attribute in ["SameSite=Lax", "Secure", "HttpOnly", "Max-Age="] {
        assert!(cookie.contains(attribute), "{attribute}: {cookie}");
    }
}

#[sqlx::test]
async fn test_logged_out_pages_and_fragments_go_to_login(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let anonymous = server.anonymous();

    let response = anonymous.get("/utilities?x=1").await;
    assert!(response.status.is_redirection());
    assert_eq!(
        response.header(header::LOCATION),
        "/login?next=%2Futilities%3Fx%3D1"
    );

    let current_url = (
        hx("hx-current-url"),
        "http://localhost:8080/folders/3?photo=9",
    );
    for (headers, location) in [
        (
            &[HX_REQUEST, current_url][..],
            "/login?next=%2Ffolders%2F3%3Fphoto%3D9",
        ),
        (&[], "/login"),
    ] {
        let response = anonymous.get_with("/ui/dialogs/shortcuts", headers).await;
        let response = response.assert_status(StatusCode::UNAUTHORIZED);
        assert_eq!(response.header(hx("hx-redirect")), location);
    }
}

#[sqlx::test]
async fn test_pages_and_fragments_render_when_logged_in(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;

    for (uri, heading) in [
        ("/", "Photos"),
        ("/folders", "Folders"),
        ("/favorites", "Favorites"),
        ("/utilities", "Utilities"),
        ("/trash", "Trash"),
        ("/duplicates", "Duplicates"),
        ("/large-files", "Large Files"),
        ("/settings", "Settings"),
        ("/upload", "Upload"),
    ] {
        let html = alice.get(uri).await.assert_status(StatusCode::OK).text();
        assert!(
            html.contains(&format!(r#"fp-topbar__heading">{heading}</h1>"#)),
            "{uri}"
        );
        assert!(html.contains("Alice"), "{uri}");
    }

    // Alice has no photos: the grid pages show their empty state
    for (uri, empty) in [
        ("/favorites", "No favorites yet"),
        ("/large-files", "No files larger than 50 MB"),
        ("/trash", "Trash is empty"),
    ] {
        let html = alice.get(uri).await.assert_status(StatusCode::OK).text();
        assert!(html.contains(empty), "{uri}");
        assert!(!html.contains("data-grid-empty hidden"), "{uri}");
    }
    let html = alice.get("/trash").await.text();
    assert!(html.contains(r#"data-testid="trash-restore-all" disabled"#));
    assert!(html.contains(r#"data-testid="trash-empty" disabled"#));

    let response = alice.get("/ui/dialogs/shortcuts").await;
    assert!(
        response
            .assert_status(StatusCode::OK)
            .text()
            .contains("Keyboard shortcuts")
    );
}

#[sqlx::test]
async fn test_not_found(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let anonymous = server.anonymous();
    let alice = server.login("alice").await;

    let response = anonymous
        .get("/missing")
        .await
        .assert_status(StatusCode::NOT_FOUND);
    assert!(response.text().contains("Page not found"));
    assert!(!response.text().contains("fp-topbar"));
    let response = alice
        .get("/missing")
        .await
        .assert_status(StatusCode::NOT_FOUND);
    assert!(response.text().contains("fp-topbar"));

    // The API keeps plain 404s
    let response = anonymous
        .get("/api/missing")
        .await
        .assert_status(StatusCode::NOT_FOUND);
    assert_eq!(response.text(), "");

    // The old folder URLs redirect
    let response = anonymous.get("/folder/7").await;
    let response = response.assert_status(StatusCode::PERMANENT_REDIRECT);
    assert_eq!(response.header(header::LOCATION), "/folders/7");
    for (client, uri) in [(&anonymous, "/folder/abc"), (&alice, "/folders/abc")] {
        let response = client.get(uri).await.assert_status(StatusCode::NOT_FOUND);
        assert!(response.text().contains("Page not found"), "{uri}");
    }
}

#[sqlx::test]
async fn test_login(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let login = async |form: &str, htmx: bool| {
        let headers = if htmx { &[HX_REQUEST][..] } else { &[] };
        server.login_with(form, headers).await
    };
    let alice = format!("user_id=alice&password={PASSWORD}");

    // The user id is normalized
    let response = login(&format!("user_id=%20Alice%20&password={PASSWORD}"), false).await;
    let response = response.assert_status(StatusCode::OK);
    assert_eq!(
        response.text(),
        r#"{"user_id":"alice","display_name":"Alice"}"#
    );

    // The JSON errors of the Android app
    login("user_id=alice&password=wrong", false)
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
    login("user_id=&password=x", false)
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    // HTMX goes to a safe `next`
    for (next, location) in [
        ("%2Ffolders%3Fq%3Da", "/folders?q=a"),
        ("%2F%2Fevil.com", "/"),
    ] {
        let response = login(&format!("{alice}&next={next}"), true).await;
        let response = response.assert_status(StatusCode::OK);
        assert_eq!(response.header(hx("hx-redirect")), location, "{next}");
    }
    let response = login("user_id=alice&password=wrong", true).await;
    let response = response.assert_status(StatusCode::OK);
    assert!(!response.headers.contains_key("hx-redirect"));
    assert!(response.text().contains("Invalid username or password."));
}

#[test]
fn test_asset_cache_control() {
    const IMMUTABLE: &str = "public, max-age=31536000, immutable";

    assert_eq!(
        asset_cache_control("/css/main.css", Some("v=abc"), "abc"),
        IMMUTABLE
    );
    assert_eq!(
        asset_cache_control("/icons/sprite.svg", Some("x=1&v=abc"), "abc"),
        IMMUTABLE
    );
    // JS modules, missing or stale versions and dev builds are always revalidated
    assert_eq!(
        asset_cache_control("/js/app.js", Some("v=abc"), "abc"),
        "no-cache"
    );
    assert_eq!(
        asset_cache_control("/css/main.css", None, "abc"),
        "no-cache"
    );
    assert_eq!(
        asset_cache_control("/css/main.css", Some("v=old"), "abc"),
        "no-cache"
    );
    assert_eq!(
        asset_cache_control("/css/main.css", Some("v=dev"), "dev"),
        "no-cache"
    );
}

#[sqlx::test]
async fn test_strict_csp_everywhere(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;

    for uri in [
        "/",
        "/upload",
        "/login",
        "/folders",
        "/api/profile",
        "/assets/js/app.js",
        "/missing",
        "/assets/js/upload/missing.js",
    ] {
        let response = alice.get(uri).await;
        let csp = response.header(header::CONTENT_SECURITY_POLICY);
        assert!(csp.contains("script-src 'self';"), "{uri}: {csp}");
    }
    let response = server.anonymous().get("/missing").await;
    assert!(
        response
            .header(header::CONTENT_SECURITY_POLICY)
            .contains("script-src 'self';")
    );
}

#[sqlx::test]
async fn test_service_worker_script(pool: SqlitePool) {
    let server = TestServer::new(pool).await;

    // Reachable logged out (the login page registers it), revalidated on every check
    let response = server
        .anonymous()
        .get("/sw.js")
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(
        response.header(header::CONTENT_TYPE),
        "text/javascript; charset=utf-8"
    );
    assert_eq!(response.header(header::CACHE_CONTROL), "no-cache");
    let body = response.text();
    assert!(body.contains("const VERSION = 'dev';"), "{body}");
    assert!(body.contains(r#"const PRECACHE = ["/assets/offline.html"];"#));
}
