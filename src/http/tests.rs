//! Router-level tests of the page shell: auth redirects, fragments, fallbacks and headers.

use crate::db::SessionStore;
use crate::http::{AppState, asset_cache_control, router};
use crate::model::user::User;
use crate::repo::users_repo::UsersRepository;
use crate::utils::password_hash::generate_hash_from_password;
use crate::utils::storage_resolver::StorageResolver;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use sqlx::SqlitePool;
use tower::ServiceExt;

async fn test_router(pool: SqlitePool) -> Router {
    let dir = tempfile::tempdir().unwrap().keep();
    let storage = StorageResolver::new(dir.join("storage"), dir.join("previews"));

    UsersRepository::new(pool.clone(), pool.clone())
        .insert_user(&User {
            id: "alice".to_string(),
            name: "Alice".to_string(),
            password_hash: generate_hash_from_password("alice-password"),
        })
        .await
        .unwrap();

    let session_store = SessionStore::new(pool.clone(), pool.clone());
    session_store.migrate().await.unwrap();

    let app_state = Box::leak(Box::new(AppState::new(pool.clone(), pool, storage)));
    router(app_state, session_store, Vec::new())
}

async fn send(router: &Router, request: Request<Body>) -> Response {
    router.clone().oneshot(request).await.unwrap()
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

fn login_request(form: &str, htmx: bool) -> Request<Body> {
    let mut request = Request::post("/api/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    if htmx {
        request = request.header("hx-request", "true");
    }
    request.body(Body::from(form.to_string())).unwrap()
}

async fn body_text(response: Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

fn header_value<'a>(response: &'a Response, name: &str) -> &'a str {
    response.headers().get(name).unwrap().to_str().unwrap()
}

/// Logs in and returns the session cookie
async fn login(router: &Router) -> String {
    let response = send(
        router,
        login_request("user_id=alice&password=alice-password", false),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = header_value(&response, "set-cookie");
    cookie.split(';').next().unwrap().to_string()
}

#[sqlx::test]
async fn test_pages_redirect_to_login(pool: SqlitePool) {
    let router = test_router(pool).await;

    let response = send(&router, get("/utilities?x=1")).await;
    assert!(response.status().is_redirection());
    assert_eq!(
        header_value(&response, "location"),
        "/login?next=%2Futilities%3Fx%3D1"
    );
}

#[sqlx::test]
async fn test_fragments_answer_401_with_hx_redirect(pool: SqlitePool) {
    let router = test_router(pool).await;

    let request = Request::get("/ui/dialogs/shortcuts")
        .header("hx-request", "true")
        .header("hx-current-url", "http://localhost:8080/folders/3?photo=9")
        .body(Body::empty())
        .unwrap();
    let response = send(&router, request).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        header_value(&response, "hx-redirect"),
        "/login?next=%2Ffolders%2F3%3Fphoto%3D9"
    );

    let response = send(&router, get("/ui/dialogs/shortcuts")).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(header_value(&response, "hx-redirect"), "/login");
}

#[sqlx::test]
async fn test_pages_and_fragments_render_when_logged_in(pool: SqlitePool) {
    let router = test_router(pool).await;
    let cookie = login(&router).await;

    for (uri, heading) in [
        ("/", "Photos"),
        ("/folders", "Folders"),
        ("/folders/1", "Folder"),
        ("/favorites", "Favorites"),
        ("/utilities", "Utilities"),
        ("/trash", "Trash"),
        ("/duplicates", "Duplicates"),
        ("/large-files", "Large Files"),
        ("/settings", "Settings"),
        ("/upload", "Upload Photos"),
    ] {
        let request = Request::get(uri)
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap();
        let response = send(&router, request).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        let body = body_text(response).await;
        assert!(
            body.contains(&format!(r#"fp-topbar__heading">{heading}</h1>"#)),
            "{uri}"
        );
        assert!(body.contains("Alice"), "{uri}");
    }

    let request = Request::get("/ui/dialogs/shortcuts")
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    let response = send(&router, request).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(body_text(response).await.contains("Keyboard shortcuts"));
}

#[sqlx::test]
async fn test_not_found(pool: SqlitePool) {
    let router = test_router(pool).await;

    let response = send(&router, get("/missing")).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(header_value(&response, "content-security-policy").contains("script-src 'self'"));
    let body = body_text(response).await;
    assert!(body.contains("Page not found"));
    assert!(!body.contains("fp-topbar"));

    let cookie = login(&router).await;
    let request = Request::get("/missing")
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    let response = send(&router, request).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(body_text(response).await.contains("fp-topbar"));

    // The API keeps plain 404s
    let response = send(&router, get("/api/missing")).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(body_text(response).await, "");
}

#[sqlx::test]
async fn test_legacy_folder_redirect(pool: SqlitePool) {
    let router = test_router(pool).await;

    let response = send(&router, get("/folder/7")).await;
    assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(header_value(&response, "location"), "/folders/7");

    let response = send(&router, get("/folder/abc")).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(body_text(response).await.contains("Page not found"));

    let cookie = login(&router).await;
    let request = Request::get("/folders/abc")
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    let response = send(&router, request).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(body_text(response).await.contains("Page not found"));
}

#[sqlx::test]
async fn test_login_normalizes_the_user_id(pool: SqlitePool) {
    let router = test_router(pool).await;

    let response = send(
        &router,
        login_request("user_id=%20Alice%20&password=alice-password", false),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_text(response).await,
        r#"{"user_id":"alice","display_name":"Alice"}"#
    );
}

#[sqlx::test]
async fn test_login_htmx_redirects_to_safe_next(pool: SqlitePool) {
    let router = test_router(pool).await;

    let response = send(
        &router,
        login_request(
            "user_id=alice&password=alice-password&next=%2Ffolders%3Fq%3Da",
            true,
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_value(&response, "hx-redirect"), "/folders?q=a");

    let response = send(
        &router,
        login_request(
            "user_id=alice&password=alice-password&next=%2F%2Fevil.com",
            true,
        ),
    )
    .await;
    assert_eq!(header_value(&response, "hx-redirect"), "/");

    let response = send(&router, login_request("user_id=alice&password=wrong", true)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get("hx-redirect").is_none());
    assert!(
        body_text(response)
            .await
            .contains("Invalid username or password.")
    );
}

#[sqlx::test]
async fn test_login_json_errors_are_unchanged(pool: SqlitePool) {
    let router = test_router(pool).await;

    let response = send(
        &router,
        login_request("user_id=alice&password=wrong", false),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = send(&router, login_request("user_id=&password=x", false)).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
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
