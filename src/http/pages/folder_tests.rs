//! Handler tests of the folders and sharing: the folder pages, their fragments and
//! the `/api` endpoints they use, driven through the whole router.

use crate::db::SessionStore;
use crate::http::{AppState, AppStateRef, router};
use crate::model::photo::Photo;
use crate::model::user::User;
use crate::repo::FolderPermissionsRepo;
use crate::repo::tests::{create_test_folder, insert_photo, photo};
use crate::utils::password_hash::generate_hash_from_password;
use crate::utils::storage_resolver::StorageResolver;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use std::sync::LazyLock;
use tempfile::TempDir;
use time::{Duration, OffsetDateTime};
use tower::ServiceExt;

const PASSWORD: &str = "password";

/// Hashing is slow in debug builds, so hash once
static PASSWORD_HASH: LazyLock<String> = LazyLock::new(|| generate_hash_from_password(PASSWORD));

struct Response {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

impl Response {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("{e}: {}", self.body))
    }
}

struct App {
    router: Router,
    state: AppStateRef,
    dir: TempDir,
}

impl App {
    /// alice, bob and carol, with an empty storage
    async fn new(pool: SqlitePool) -> Self {
        let dir = TempDir::new().unwrap();
        let storage = StorageResolver::new(dir.path().join("storage"), dir.path().join("previews"));
        let session_store = SessionStore::new(pool.clone(), pool.clone());
        session_store.migrate().await.unwrap();
        let state: AppStateRef = Box::leak(Box::new(AppState::new(pool.clone(), pool, storage)));

        for (id, name) in [("alice", "Alice"), ("bob", "Bob"), ("carol", "Carol")] {
            state
                .users_repo
                .insert_user(&User {
                    id: id.to_string(),
                    name: name.to_string(),
                    password_hash: PASSWORD_HASH.clone(),
                })
                .await
                .unwrap();
        }

        Self {
            router: router(state, session_store, Vec::new()),
            state,
            dir,
        }
    }

    fn pool(&self) -> &SqlitePool {
        &self.state.write_pool
    }

    async fn send(&self, request: Request<Body>) -> Response {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        Response {
            status,
            headers,
            body: String::from_utf8_lossy(&body).into_owned(),
        }
    }

    /// Logs in and returns the session cookie
    async fn login(&self, user_id: &str) -> String {
        let response = self
            .send(
                Request::post("/api/login")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(format!("user_id={user_id}&password={PASSWORD}")))
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "{}", response.body);
        response.headers[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string()
    }

    async fn request(
        &self,
        cookie: &str,
        method: Method,
        uri: &str,
        json_body: Option<Value>,
    ) -> Response {
        let builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::COOKIE, cookie);
        let request = match json_body {
            Some(body) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string())),
            None => builder.body(Body::empty()),
        };
        self.send(request.unwrap()).await
    }

    async fn get(&self, cookie: &str, uri: &str) -> Response {
        self.request(cookie, Method::GET, uri, None).await
    }

    async fn delete_folder(&self, cookie: &str, id: i64) -> Response {
        self.request(cookie, Method::DELETE, &format!("/api/folders/{id}"), None)
            .await
    }

    async fn put_share(&self, cookie: &str, id: i64, body: Value) -> Response {
        self.request(
            cookie,
            Method::PUT,
            &format!("/api/sharing/{id}"),
            Some(body),
        )
        .await
    }

    async fn folder(&self, owner: Option<&str>, name: &str) -> i64 {
        create_test_folder(self.pool(), owner, name).await.id
    }

    async fn photo(&self, owner: Option<&str>, folder_id: i64, name: &str, trashed: bool) -> i64 {
        let photo = Photo {
            folder_id: Some(folder_id),
            trashed_on: trashed.then(OffsetDateTime::now_utc),
            ..photo(owner, name)
        };
        insert_photo(self.pool(), photo).await.id
    }
}

fn has_testid(html: &str, testid: &str) -> bool {
    html.contains(&format!(r#"data-testid="{testid}""#))
}

#[sqlx::test]
async fn test_delete_folder(pool: SqlitePool) {
    let app = App::new(pool).await;
    let alice = app.login("alice").await;
    let bob = app.login("bob").await;

    let empty = app.folder(Some("alice"), "empty").await;
    let trashed_only = app.folder(Some("alice"), "trashed").await;
    app.photo(Some("alice"), trashed_only, "t.jpg", true).await;
    let full = app.folder(Some("alice"), "full").await;
    app.photo(Some("alice"), full, "a.jpg", false).await;
    let shared = app.folder(Some("alice"), "shared").await;
    app.pool()
        .create_share(shared, Some("bob"), true, true, None)
        .await
        .unwrap();
    let family = app.folder(None, "family").await;

    let directory = app.dir.path().join("storage/alice/empty");
    std::fs::create_dir_all(&directory).unwrap();

    // Not logged out
    let response = app
        .send(
            Request::delete(format!("/api/folders/{empty}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);

    // Photos, even trashed ones, keep the folder
    let response = app.delete_folder(&alice, full).await;
    assert_eq!(response.status, StatusCode::CONFLICT);
    assert_eq!(
        response.body,
        "Move or delete the photos in this folder first"
    );
    assert_eq!(
        app.delete_folder(&alice, trashed_only).await.status,
        StatusCode::CONFLICT
    );

    // Other people's folders: invisible, or visible through a share but not theirs to delete
    assert_eq!(
        app.delete_folder(&bob, empty).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.delete_folder(&bob, shared).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.delete_folder(&alice, 99999).await.status,
        StatusCode::NOT_FOUND
    );

    let response = app.delete_folder(&alice, empty).await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    assert!(!directory.exists());
    assert_eq!(
        app.delete_folder(&alice, empty).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.get(&alice, &format!("/folders/{empty}")).await.status,
        StatusCode::NOT_FOUND
    );

    // Its shares go with it, others' shares stay
    let response = app.delete_folder(&alice, shared).await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    assert!(
        app.pool()
            .get_shares_for_folder(shared)
            .await
            .unwrap()
            .is_empty()
    );

    // Anyone can delete an empty public folder
    assert_eq!(
        app.delete_folder(&bob, family).await.status,
        StatusCode::NO_CONTENT
    );
}

#[sqlx::test]
async fn test_delete_folder_without_a_directory(pool: SqlitePool) {
    let app = App::new(pool).await;
    let alice = app.login("alice").await;
    let folder = app.folder(Some("alice"), "never-on-disk").await;

    let response = app
        .request(
            &alice,
            Method::DELETE,
            &format!("/api/folders/{folder}"),
            None,
        )
        .await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
}

#[sqlx::test]
async fn test_share_expiry(pool: SqlitePool) {
    let app = App::new(pool).await;
    let alice = app.login("alice").await;
    let bob = app.login("bob").await;
    let folder = app.folder(Some("alice"), "shared").await;

    let in_a_week = (OffsetDateTime::now_utc() + Duration::days(7)).unix_timestamp();
    let response = app
        .request(
            &alice,
            Method::POST,
            "/api/sharing",
            Some(json!({"folder_id": folder, "grantee_id": "bob", "expires_at": in_a_week})),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    let share = response.json();
    let id = share["id"].as_i64().unwrap();
    assert_eq!(share["expires_at"], in_a_week);
    assert_eq!(share["can_upload"], false);

    // The Android app sends just the flags: the expiry stays
    let response = app
        .put_share(&alice, id, json!({"can_upload": true, "can_delete": false}))
        .await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.json()["expires_at"], in_a_week);
    assert_eq!(response.json()["can_upload"], true);

    // A new expiry
    let in_a_month = (OffsetDateTime::now_utc() + Duration::days(30)).unix_timestamp();
    let response = app
        .put_share(
            &alice,
            id,
            json!({"can_upload": true, "can_delete": false, "expires_at": in_a_month}),
        )
        .await;
    assert_eq!(response.json()["expires_at"], in_a_month);

    // It has to be in the future, and a rejected request changes nothing
    let response = app
        .put_share(
            &alice,
            id,
            json!({"can_upload": false, "can_delete": false, "expires_at": 1_000_000}),
        )
        .await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert_eq!(response.body, "Expiry must be in the future");
    let shares = app
        .get(&alice, &format!("/api/sharing/folder/{folder}"))
        .await
        .json();
    assert_eq!(shares[0]["expires_at"], in_a_month);
    assert_eq!(shares[0]["can_upload"], true);

    // null removes it
    let response = app
        .put_share(
            &alice,
            id,
            json!({"can_upload": true, "can_delete": false, "expires_at": null}),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.json()["expires_at"], Value::Null);

    // Only the owner changes shares
    let response = app
        .put_share(
            &bob,
            id,
            json!({"can_upload": true, "expires_at": in_a_week}),
        )
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);

    // Creating a share that has already expired is a bad request, not a database error
    let response = app
        .request(
            &alice,
            Method::POST,
            "/api/sharing",
            Some(json!({"folder_id": folder, "grantee_id": "carol", "expires_at": 1_000_000})),
        )
        .await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert_eq!(response.body, "Expiry must be in the future");
}

#[sqlx::test]
async fn test_folders_page_lists_accessible_folders(pool: SqlitePool) {
    let app = App::new(pool).await;
    let bob = app.login("bob").await;

    let holiday = app.folder(Some("alice"), "Holiday").await;
    app.photo(Some("alice"), holiday, "a.jpg", false).await;
    app.photo(Some("alice"), holiday, "b.jpg", false).await;
    app.photo(Some("alice"), holiday, "c.jpg", true).await;
    app.pool()
        .create_share(holiday, Some("bob"), false, false, None)
        .await
        .unwrap();
    app.folder(Some("alice"), "Private").await;
    let trip = app.folder(None, "Trip").await;
    app.photo(None, trip, "t.jpg", false).await;
    app.folder(Some("bob"), "Vacanță").await;

    let response = app.get(&bob, "/folders").await;
    assert_eq!(response.status, StatusCode::OK);
    let html = response.body;
    assert!(html.contains("Holiday") && html.contains("Trip") && html.contains("Vacanță"));
    assert!(!html.contains("Private"));
    assert!(html.contains(&format!(r#"href="/folders/{holiday}""#)));
    // The trashed photo isn't counted
    assert!(html.contains("2 photos"));
    assert!(html.contains("0 photos"));
    assert!(has_testid(&html, "new-folder"));

    // Overrides of the saved prefs
    let html = app.get(&bob, "/folders?filter=shared&view=list").await.body;
    assert!(html.contains("Holiday") && !html.contains("Trip") && !html.contains("Vacanță"));
    assert!(html.contains("2 photos • Shared • Alice"));
    let html = app
        .get(&bob, "/folders?q=vacanta&sort=name_desc")
        .await
        .body;
    assert!(html.contains(r#"value="vacanta""#));

    // Saved prefs come from the cookie
    let request = Request::get("/folders")
        .header(header::COOKIE, format!("{bob}; fp_prefs=filter=family"))
        .body(Body::empty())
        .unwrap();
    let html = app.send(request).await.body;
    assert!(html.contains("Trip") && !html.contains("Holiday"));
}

#[sqlx::test]
async fn test_folder_list_fragment(pool: SqlitePool) {
    let app = App::new(pool).await;
    let bob = app.login("bob").await;

    let holiday = app.folder(Some("alice"), "Holiday").await;
    app.pool()
        .create_share(holiday, Some("bob"), false, false, None)
        .await
        .unwrap();
    let editable = app.folder(Some("alice"), "Editable").await;
    app.pool()
        .create_share(editable, Some("bob"), true, false, None)
        .await
        .unwrap();
    app.folder(Some("bob"), "Mine").await;

    let html = app.get(&bob, "/ui/folders/list?sort=name_asc").await.body;
    let positions: Vec<_> = ["Editable", "Holiday", "Mine"]
        .iter()
        .map(|name| html.find(&format!(r#"data-name="{name}""#)).unwrap())
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(html.contains(r#" id="folder-list""#));
    assert!(html.contains("<a href="));

    let html = app.get(&bob, "/ui/folders/list?sort=name_desc").await.body;
    assert!(html.find("Mine").unwrap() < html.find("Editable").unwrap());

    // The picker only offers folders the user can upload to, as buttons
    let html = app.get(&bob, "/ui/folders/list?picker=true").await.body;
    assert!(html.contains("Editable") && html.contains("Mine") && !html.contains("Holiday"));
    assert!(html.contains("<button"));
    assert!(!html.contains(r#" id="folder-list""#));

    let html = app
        .get(&bob, "/ui/folders/list?filter=shared&picker=true")
        .await
        .body;
    assert!(html.contains("Editable") && !html.contains("Mine"));

    let html = app.get(&bob, "/ui/folders/list?filter=family").await.body;
    assert!(html.contains("No folders yet"));
    let html = app
        .get(&bob, "/ui/folders/list?filter=family&picker=true")
        .await
        .body;
    assert!(html.contains("No folders to choose"));

    // Logged out: 401 with a redirect for HTMX
    let response = app
        .send(
            Request::get("/ui/folders/list")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    assert!(response.headers.contains_key("hx-redirect"));
}

#[sqlx::test]
async fn test_folder_page_controls_follow_permissions(pool: SqlitePool) {
    let app = App::new(pool).await;
    let alice = app.login("alice").await;
    let bob = app.login("bob").await;
    let carol = app.login("carol").await;

    let holiday = app.folder(Some("alice"), "Holiday").await;
    app.photo(Some("alice"), holiday, "a.jpg", false).await;
    app.pool()
        .create_share(holiday, Some("bob"), false, false, None)
        .await
        .unwrap();
    let editable = app.folder(Some("alice"), "Editable").await;
    app.pool()
        .create_share(editable, Some("bob"), true, true, None)
        .await
        .unwrap();
    app.pool()
        .create_share(editable, Some("carol"), false, false, None)
        .await
        .unwrap();
    let family = app.folder(None, "Family trip").await;
    let empty = app.folder(Some("alice"), "Empty").await;

    // The owner: everything, but no delete while the folder has photos
    let html = app.get(&alice, &format!("/folders/{holiday}")).await.body;
    for testid in ["folder-share", "folder-edit", "folder-upload"] {
        assert!(has_testid(&html, testid), "{testid}");
    }
    for testid in ["folder-delete", "folder-access"] {
        assert!(!has_testid(&html, testid), "{testid}");
    }
    // The folder's photo grid, with the empty state hidden until its last photo is removed
    assert_eq!(html.matches(r#"data-testid="photo-tile""#).count(), 1);
    assert!(html.contains(r#"data-grid-empty hidden"#));
    assert!(has_testid(&html, "zoom-in"));
    assert!(html.contains("1 photo<"));
    assert!(html.contains("Shared (1)"));
    assert!(html.contains(&format!(r#"data-context="folder:{holiday}""#)));

    let html = app.get(&alice, &format!("/folders/{empty}")).await.body;
    assert!(has_testid(&html, "folder-delete"));
    assert!(has_testid(&html, "grid-empty-upload"));
    assert!(!html.contains(r#"data-grid-empty hidden"#));
    assert!(html.contains("This folder is empty"));
    assert!(html.contains("Personal"));

    // A view-only grantee: no management, no upload
    let html = app.get(&bob, &format!("/folders/{holiday}")).await.body;
    for testid in [
        "folder-share",
        "folder-edit",
        "folder-delete",
        "folder-upload",
    ] {
        assert!(!has_testid(&html, testid), "{testid}");
    }
    assert!(has_testid(&html, "folder-access"));
    assert!(html.contains("Shared by Alice"));
    assert_eq!(html.matches(r#"data-testid="photo-tile""#).count(), 1);
    assert!(!has_testid(&html, "grid-empty-upload"));

    // Uploading and deleting photos are share permissions, managing the folder is not
    let html = app.get(&bob, &format!("/folders/{editable}")).await.body;
    assert!(has_testid(&html, "folder-upload"));
    assert!(!has_testid(&html, "folder-edit") && !has_testid(&html, "folder-share"));
    assert!(html.contains(r#"data-can-delete="true""#));
    let html = app.get(&carol, &format!("/folders/{editable}")).await.body;
    assert!(html.contains(r#"data-can-delete="false""#));

    // Public folders can be managed (renamed, deleted when empty) by anyone, but not shared
    let html = app.get(&carol, &format!("/folders/{family}")).await.body;
    assert!(has_testid(&html, "folder-edit") && has_testid(&html, "folder-upload"));
    assert!(has_testid(&html, "folder-delete"));
    assert!(!has_testid(&html, "folder-share") && !has_testid(&html, "folder-access"));
    assert!(html.contains("Family<"));

    // Not accessible, not a folder
    assert_eq!(
        app.get(&carol, &format!("/folders/{holiday}")).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.get(&bob, &format!("/folders/{empty}")).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.get(&bob, "/folders/99999").await.status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
async fn test_expired_share_hides_the_folder(pool: SqlitePool) {
    let app = App::new(pool).await;
    let bob = app.login("bob").await;
    let folder = app.folder(Some("alice"), "Holiday").await;
    let share = app
        .pool()
        .create_share(
            folder,
            Some("bob"),
            false,
            false,
            Some(OffsetDateTime::now_utc() + Duration::days(1)),
        )
        .await
        .unwrap();

    assert_eq!(
        app.get(&bob, &format!("/folders/{folder}")).await.status,
        StatusCode::OK
    );

    sqlx::query("update folder_permissions set created_at = ?, expires_at = ? where id = ?")
        .bind(OffsetDateTime::now_utc() - Duration::days(10))
        .bind(OffsetDateTime::now_utc() - Duration::days(2))
        .bind(share.id)
        .execute(app.pool())
        .await
        .unwrap();

    assert_eq!(
        app.get(&bob, &format!("/folders/{folder}")).await.status,
        StatusCode::NOT_FOUND
    );
    assert!(!app.get(&bob, "/folders").await.body.contains("Holiday"));
}

#[sqlx::test]
async fn test_folder_name_dialog(pool: SqlitePool) {
    let app = App::new(pool).await;
    let alice = app.login("alice").await;
    let bob = app.login("bob").await;

    let mine = app.folder(Some("alice"), "Holiday").await;
    let family = app.folder(None, "Trip").await;
    app.pool()
        .create_share(mine, Some("bob"), true, true, None)
        .await
        .unwrap();

    let html = app.get(&alice, "/ui/dialogs/folder-name").await.body;
    assert!(html.contains("New folder"));
    assert!(!html.contains("data-renaming"));
    assert!(html.contains(r#"data-initial-public="false""#));
    assert!(html.contains("maxlength=\"100\""));

    let html = app
        .get(&alice, "/ui/dialogs/folder-name?name=Foo&public=true")
        .await
        .body;
    assert!(html.contains(r#"value="Foo""#));
    assert!(html.contains(r#"data-initial-public="true""#));

    let html = app
        .get(&alice, &format!("/ui/dialogs/folder-name?folder_id={mine}"))
        .await
        .body;
    assert!(html.contains("Edit folder") && html.contains("data-renaming"));
    assert!(html.contains(r#"value="Holiday""#));

    let html = app
        .get(&bob, &format!("/ui/dialogs/folder-name?folder_id={family}"))
        .await
        .body;
    assert!(html.contains(r#"value="Trip""#));
    assert!(html.contains(r#"data-initial-public="true""#));

    // Shared with bob (even with every permission) or hidden: not his to rename
    let response = app
        .get(&bob, &format!("/ui/dialogs/folder-name?folder_id={mine}"))
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    let response = app
        .get(&bob, "/ui/dialogs/folder-name?folder_id=9999")
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn test_sharing_dialog(pool: SqlitePool) {
    let app = App::new(pool).await;
    let alice = app.login("alice").await;
    let bob = app.login("bob").await;
    let carol = app.login("carol").await;

    let folder = app.folder(Some("alice"), "Holiday").await;
    let in_a_week = OffsetDateTime::now_utc() + Duration::days(7);
    app.pool()
        .create_share(folder, Some("bob"), true, true, Some(in_a_week))
        .await
        .unwrap();
    // Link shares aren't managed here
    app.pool()
        .create_share(folder, None, false, false, None)
        .await
        .unwrap();
    let family = app.folder(None, "Trip").await;

    let owner_view = app
        .get(&alice, &format!("/ui/dialogs/sharing/{folder}"))
        .await
        .body;
    assert!(owner_view.contains("Manage access"));
    assert!(has_testid(&owner_view, "sharing-owner") && owner_view.contains("Owner"));
    assert_eq!(
        owner_view
            .matches(r#"data-testid="sharing-member""#)
            .count(),
        1
    );
    assert!(owner_view.contains("Can upload &#38; delete"));
    assert!(owner_view.contains(&format!(
        "Expires {}",
        crate::http::views::format_date(in_a_week, crate::http::views::DateStyle::Day)
    )));
    assert!(has_testid(&owner_view, "sharing-remove"));
    assert!(has_testid(&owner_view, "sharing-permission-upload"));
    // Only carol can still be added: not the owner, not bob
    assert!(has_testid(&owner_view, "sharing-add"));
    assert_eq!(
        owner_view
            .matches(r#"data-testid="sharing-add-user""#)
            .count(),
        1
    );
    assert!(owner_view.contains(r#"data-user-id="carol""#));
    assert!(!owner_view.contains(r#"data-user-id="alice""#));
    assert!(owner_view.contains(r#"data-member-count="1""#));

    // An expired member is listed (to renew or remove), but not counted, like the folder badge
    let expired = app
        .pool()
        .create_share(folder, Some("carol"), false, false, Some(in_a_week))
        .await
        .unwrap();
    sqlx::query("update folder_permissions set created_at = ?, expires_at = ? where id = ?")
        .bind(OffsetDateTime::now_utc() - Duration::days(10))
        .bind(OffsetDateTime::now_utc() - Duration::seconds(30))
        .bind(expired.id)
        .execute(app.pool())
        .await
        .unwrap();
    let owner_view = app
        .get(&alice, &format!("/ui/dialogs/sharing/{folder}"))
        .await
        .body;
    assert_eq!(
        owner_view
            .matches(r#"data-testid="sharing-member""#)
            .count(),
        2
    );
    assert!(owner_view.contains("Expired "));
    assert!(owner_view.contains(r#"data-member-count="1""#));
    let page = app.get(&alice, &format!("/folders/{folder}")).await.body;
    assert!(page.contains("Shared (1)"));
    app.pool().delete_share(expired.id, "alice").await.unwrap();

    // A grantee sees who has access, read only
    let grantee_view = app
        .get(&bob, &format!("/ui/dialogs/sharing/{folder}"))
        .await
        .body;
    assert!(grantee_view.contains("People with access"));
    assert_eq!(
        grantee_view
            .matches(r#"data-testid="sharing-member""#)
            .count(),
        1
    );
    for testid in [
        "sharing-remove",
        "sharing-add",
        "sharing-member-menu",
        "sharing-permission-view",
    ] {
        assert!(!has_testid(&grantee_view, testid), "{testid}");
    }

    // Nobody else does
    assert_eq!(
        app.get(&carol, &format!("/ui/dialogs/sharing/{folder}"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    // Public folders show "Everyone in Family" and offer no changes
    let public_view = app
        .get(&alice, &format!("/ui/dialogs/sharing/{family}"))
        .await
        .body;
    assert!(public_view.contains("Everyone in Family") && public_view.contains("Shared access"));
    assert!(!has_testid(&public_view, "sharing-add"));
}

#[sqlx::test]
async fn test_destination_picker_and_move_dialog(pool: SqlitePool) {
    let app = App::new(pool).await;
    let bob = app.login("bob").await;

    let view_only = app.folder(Some("alice"), "Read only").await;
    app.pool()
        .create_share(view_only, Some("bob"), false, false, None)
        .await
        .unwrap();
    let editable = app.folder(Some("alice"), "Editable").await;
    app.pool()
        .create_share(editable, Some("bob"), true, false, None)
        .await
        .unwrap();
    let mine = app.folder(Some("bob"), "Mine").await;

    let html = app.get(&bob, "/ui/destination-picker").await.body;
    assert!(html.contains(r#"data-mode="move""#));
    assert!(html.contains("destination-picker"));
    assert!(html.contains("Editable") && html.contains("Mine") && !html.contains("Read only"));
    assert!(has_testid(&html, "dest-new"));
    assert!(html.contains(r#"data-slot="showcase""#));
    assert!(!html.contains("data-initial-folder-id"));

    let html = app
        .get(
            &bob,
            &format!("/ui/destination-picker?mode=upload&folder_id={editable}"),
        )
        .await
        .body;
    assert!(html.contains(r#"data-mode="upload""#));
    assert!(html.contains(&format!(r#"data-initial-folder-id="{editable}""#)));
    assert!(html.contains(r#"data-initial-folder-type="shared""#));

    // A folder the user can't upload to isn't preselected
    let html = app
        .get(
            &bob,
            &format!("/ui/destination-picker?folder_id={view_only}"),
        )
        .await
        .body;
    assert!(!html.contains("data-initial-folder-id"));
    let html = app
        .get(&bob, &format!("/ui/destination-picker?folder_id={mine}"))
        .await
        .body;
    assert!(html.contains(r#"data-initial-folder-public="false""#));

    let html = app.get(&bob, "/ui/dialogs/move?count=3").await.body;
    assert!(html.contains("Move 3 photos"));
    assert!(has_testid(&html, "move-confirm"));
    assert!(html.contains("Move photos"));
    let html = app.get(&bob, "/ui/dialogs/move?count=1").await.body;
    assert!(html.contains("Move 1 photo<"));
    let html = app.get(&bob, "/ui/dialogs/move").await.body;
    assert!(html.contains(">Move photos<"));

    let response = app
        .send(
            Request::get("/ui/dialogs/move")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
}
