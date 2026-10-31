//! Handler tests of the folders and sharing: the folder pages, their fragments and
//! the `/api` endpoints they use, driven through the whole router.

use crate::http::api::tests::{TestResponse, TestServer};
use crate::model::folder_permission::FolderPermission;
use crate::model::photo::Photo;
use crate::repo::FolderPermissionsRepo;
use crate::repo::tests::{create_test_folder, insert_photo, photo};
use axum::http::{HeaderName, Method, StatusCode, header};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use std::sync::atomic::{AtomicU32, Ordering};
use time::{Duration, OffsetDateTime};

async fn folder(server: &TestServer, owner: Option<&str>, name: &str) -> i64 {
    create_test_folder(server.pool(), owner, name).await.id
}

async fn add_photo(server: &TestServer, owner: Option<&str>, folder_id: i64, trashed: bool) {
    let photo = Photo {
        folder_id: Some(folder_id),
        trashed_on: trashed.then(OffsetDateTime::now_utc),
        ..photo(
            owner,
            &format!("{}.jpg", NEXT_PHOTO.fetch_add(1, Ordering::Relaxed)),
        )
    };
    insert_photo(server.pool(), photo).await;
}

static NEXT_PHOTO: AtomicU32 = AtomicU32::new(0);

async fn share(
    server: &TestServer,
    folder_id: i64,
    grantee: &str,
    can_upload: bool,
    can_delete: bool,
) -> FolderPermission {
    server
        .pool()
        .create_share(folder_id, Some(grantee), can_upload, can_delete, None)
        .await
        .unwrap()
}

/// Moves the share into the past, so it expired `ago`
async fn expire(server: &TestServer, share_id: i64, ago: Duration) {
    sqlx::query("update folder_permissions set created_at = ?, expires_at = ? where id = ?")
        .bind(OffsetDateTime::now_utc() - Duration::days(10))
        .bind(OffsetDateTime::now_utc() - ago)
        .bind(share_id)
        .execute(server.pool())
        .await
        .unwrap();
}

async fn folder_name(server: &TestServer, id: i64) -> Option<String> {
    sqlx::query_scalar("select name from folders where id = ?")
        .bind(id)
        .fetch_optional(server.pool())
        .await
        .unwrap()
}

fn hx(name: &'static str) -> HeaderName {
    HeaderName::from_static(name)
}

/// The id of the folder an `HX-Redirect: /folders/{id}` points at
fn redirected_folder(response: &TestResponse) -> i64 {
    let location = response.header(hx("hx-redirect"));
    location.strip_prefix("/folders/").unwrap().parse().unwrap()
}

#[sqlx::test]
async fn test_delete_folder(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;
    let bob = server.login("bob").await;

    let empty = folder(&server, Some("alice"), "empty").await;
    let trashed_only = folder(&server, Some("alice"), "trashed").await;
    add_photo(&server, Some("alice"), trashed_only, true).await;
    let full = folder(&server, Some("alice"), "full").await;
    add_photo(&server, Some("alice"), full, false).await;
    let shared = folder(&server, Some("alice"), "shared").await;
    share(&server, shared, "bob", true, true).await;
    let family = folder(&server, None, "family").await;
    let never_on_disk = folder(&server, Some("alice"), "never-on-disk").await;

    let directory = server
        .state
        .storage
        .resolve_folder(Some("alice"), Some("empty"));
    std::fs::create_dir_all(&directory).unwrap();
    let delete = |id: i64| format!("/api/folders/{id}");

    server
        .anonymous()
        .delete(&delete(empty))
        .await
        .assert_status(StatusCode::UNAUTHORIZED);

    // Photos, even trashed ones, keep the folder
    let response = alice
        .delete(&delete(full))
        .await
        .assert_status(StatusCode::CONFLICT);
    assert_eq!(
        response.text(),
        "Move or delete the photos in this folder first"
    );
    alice
        .delete(&delete(trashed_only))
        .await
        .assert_status(StatusCode::CONFLICT);

    // Other people's folders: invisible, or visible through a share but not theirs to delete
    bob.delete(&delete(empty))
        .await
        .assert_status(StatusCode::NOT_FOUND);
    bob.delete(&delete(shared))
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
    alice
        .delete(&delete(99999))
        .await
        .assert_status(StatusCode::NOT_FOUND);

    alice
        .delete(&delete(empty))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert!(!directory.exists());
    alice
        .delete(&delete(empty))
        .await
        .assert_status(StatusCode::NOT_FOUND);
    alice
        .get(&format!("/folders/{empty}"))
        .await
        .assert_status(StatusCode::NOT_FOUND);
    alice
        .delete(&delete(never_on_disk))
        .await
        .assert_status(StatusCode::NO_CONTENT);

    // Its shares go with it
    alice
        .delete(&delete(shared))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert!(
        server
            .pool()
            .get_shares_for_folder(shared)
            .await
            .unwrap()
            .is_empty()
    );

    // Anyone can delete an empty public folder
    bob.delete(&delete(family))
        .await
        .assert_status(StatusCode::NO_CONTENT);
}

#[sqlx::test]
async fn test_share_expiry(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;
    let bob = server.login("bob").await;
    let folder = folder(&server, Some("alice"), "shared").await;
    let in_days = |days| (OffsetDateTime::now_utc() + Duration::days(days)).unix_timestamp();
    let (in_a_week, in_a_month) = (in_days(7), in_days(30));

    let body = json!({"folder_id": folder, "grantee_id": "bob", "expires_at": in_a_week});
    let share = alice
        .post("/api/sharing", body)
        .await
        .assert_status(StatusCode::OK)
        .json();
    assert_eq!(share["expires_at"], in_a_week);
    assert_eq!(share["can_upload"], false);
    let uri = format!("/api/sharing/{}", share["id"]);

    // The Android app sends just the flags: the expiry stays
    let share = alice
        .put(&uri, json!({"can_upload": true, "can_delete": false}))
        .await
        .assert_status(StatusCode::OK)
        .json();
    assert_eq!(share["expires_at"], in_a_week);
    assert_eq!(share["can_upload"], true);

    let body = json!({"can_upload": true, "can_delete": false, "expires_at": in_a_month});
    assert_eq!(alice.put(&uri, body).await.json()["expires_at"], in_a_month);

    // It has to be in the future, and a rejected request changes nothing
    let body = json!({"can_upload": false, "can_delete": false, "expires_at": 1_000_000});
    let response = alice
        .put(&uri, body)
        .await
        .assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(response.text(), "Expiry must be in the future");
    let shares = alice
        .get(&format!("/api/sharing/folder/{folder}"))
        .await
        .json();
    assert_eq!(shares[0]["expires_at"], in_a_month);
    assert_eq!(shares[0]["can_upload"], true);

    // null removes it
    let body = json!({"can_upload": true, "can_delete": false, "expires_at": null});
    let share = alice
        .put(&uri, body)
        .await
        .assert_status(StatusCode::OK)
        .json();
    assert_eq!(share["expires_at"], Value::Null);

    // Only the owner changes shares
    let body = json!({"can_upload": true, "expires_at": in_a_week});
    bob.put(&uri, body)
        .await
        .assert_status(StatusCode::NOT_FOUND);

    // Creating a share that has already expired is a bad request, not a database error
    let body = json!({"folder_id": folder, "grantee_id": "carol", "expires_at": 1_000_000});
    let response = alice.post("/api/sharing", body).await;
    let response = response.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(response.text(), "Expiry must be in the future");
}

/// Who sees which controls of a folder page, and who can open it at all. The browser specs
/// drive these controls; this is the permission matrix behind them.
#[sqlx::test]
async fn test_folder_page_permissions(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;
    let bob = server.login("bob").await;
    let carol = server.login("carol").await;

    let holiday = folder(&server, Some("alice"), "Holiday").await;
    add_photo(&server, Some("alice"), holiday, false).await;
    share(&server, holiday, "bob", false, false).await;
    let editable = folder(&server, Some("alice"), "Editable").await;
    share(&server, editable, "bob", true, true).await;
    share(&server, editable, "carol", false, false).await;
    let family = folder(&server, None, "Family trip").await;
    let empty = folder(&server, Some("alice"), "Empty").await;

    // (viewer, folder, controls shown, controls hidden)
    let cases = [
        // The owner: everything, but no delete while the folder has photos
        (
            &alice,
            holiday,
            &["folder-share", "folder-edit", "folder-upload"][..],
            &["folder-delete", "folder-access"][..],
        ),
        (&alice, empty, &["folder-delete", "grid-empty-upload"], &[]),
        // A view-only grantee: no management, no upload
        (
            &bob,
            holiday,
            &["folder-access"],
            &[
                "folder-share",
                "folder-edit",
                "folder-delete",
                "folder-upload",
                "grid-empty-upload",
            ],
        ),
        // Uploading and deleting photos are share permissions, managing the folder is not
        (
            &bob,
            editable,
            &["folder-upload"],
            &["folder-edit", "folder-share"],
        ),
        // Public folders can be managed (renamed, deleted when empty) by anyone, but not shared
        (
            &carol,
            family,
            &["folder-edit", "folder-upload", "folder-delete"],
            &["folder-share", "folder-access"],
        ),
    ];
    for (client, id, shown, hidden) in cases {
        let response = client.get(&format!("/folders/{id}")).await;
        for testid in shown {
            assert!(response.has_testid(testid), "{id}: {testid}");
        }
        for testid in hidden {
            assert!(!response.has_testid(testid), "{id}: {testid}");
        }
    }
    for (client, can_delete) in [(&bob, "true"), (&carol, "false")] {
        let html = client.get(&format!("/folders/{editable}")).await.text();
        assert!(html.contains(&format!(r#"data-can-delete="{can_delete}""#)));
    }

    // Members see the sharing dialog read only, nobody else sees it
    let dialog = format!("/ui/dialogs/sharing/{holiday}");
    let response = bob.get(&dialog).await.assert_status(StatusCode::OK);
    assert!(!response.has_testid("sharing-add") && !response.has_testid("sharing-remove"));
    carol
        .get(&dialog)
        .await
        .assert_status(StatusCode::NOT_FOUND);

    // Not accessible, not a folder
    for (client, id) in [(&carol, holiday), (&bob, empty), (&bob, 99999)] {
        let response = client.get(&format!("/folders/{id}")).await;
        response.assert_status(StatusCode::NOT_FOUND);
    }
}

#[sqlx::test]
async fn test_expired_share(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;
    let bob = server.login("bob").await;
    let folder = folder(&server, Some("alice"), "Holiday").await;
    let share = share(&server, folder, "bob", false, false).await;
    let uri = format!("/folders/{folder}");

    bob.get(&uri).await.assert_status(StatusCode::OK);
    expire(&server, share.id, Duration::days(2)).await;
    bob.get(&uri).await.assert_status(StatusCode::NOT_FOUND);
    assert!(!bob.get("/folders").await.text().contains("Holiday"));

    // The owner still sees bob (to renew or remove him), but he no longer counts
    let response = alice.get(&format!("/ui/dialogs/sharing/{folder}")).await;
    assert_eq!(response.count_testid("sharing-member"), 1);
    let html = response.text();
    assert!(html.contains("Expired ") && html.contains(r#"data-member-count="0""#));
    assert!(!alice.get(&uri).await.text().contains("Shared ("));
}

#[sqlx::test]
async fn test_destination_picker_offers_upload_folders_only(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let bob = server.login("bob").await;

    let view_only = folder(&server, Some("alice"), "Read only").await;
    share(&server, view_only, "bob", false, false).await;
    let editable = folder(&server, Some("alice"), "Editable").await;
    share(&server, editable, "bob", true, false).await;

    let html = bob.get("/ui/destination-picker").await.text();
    assert!(html.contains("Editable") && !html.contains("Read only"));
    for (id, preselected) in [(editable, true), (view_only, false)] {
        let html = bob
            .get(&format!("/ui/destination-picker?folder_id={id}"))
            .await
            .text();
        assert_eq!(html.contains("data-initial-folder-id"), preselected, "{id}");
    }
}

#[sqlx::test]
async fn test_ui_create_folder_visibility(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;
    let bob = server.login("bob").await;

    for (visibility, bob_status) in [
        ("personal", StatusCode::NOT_FOUND),
        ("family", StatusCode::OK),
    ] {
        let body = format!("name={visibility}&folder-visibility={visibility}");
        let response = alice.form(Method::POST, "/ui/folders", &body).await;
        let id = redirected_folder(&response.assert_status(StatusCode::OK));
        assert_eq!(folder_name(&server, id).await.as_deref(), Some(visibility));
        bob.get(&format!("/folders/{id}"))
            .await
            .assert_status(bob_status);
    }
}

/// Only the owner renames or deletes a private folder, even a grantee with every permission;
/// anybody renames a public one
#[sqlx::test]
async fn test_ui_folder_changes_follow_permissions(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;
    let bob = server.login("bob").await;

    let shared = folder(&server, Some("alice"), "Shared").await;
    share(&server, shared, "bob", true, true).await;
    let private = folder(&server, Some("alice"), "Private").await;
    let family = folder(&server, None, "Trip").await;
    let rename = "name=Mine&folder-visibility=family";

    for id in [shared, private, 9999] {
        for uri in [
            format!("/ui/dialogs/folder-name?folder_id={id}"),
            format!("/ui/dialogs/folder-delete/{id}"),
        ] {
            bob.get(&uri).await.assert_status(StatusCode::NOT_FOUND);
        }
        let uri = format!("/ui/folders/{id}");
        let response = bob.form(Method::POST, &uri, rename).await;
        response.assert_status(StatusCode::NOT_FOUND);
        let response = bob.form(Method::DELETE, &uri, "").await;
        response.assert_status(StatusCode::NOT_FOUND);
    }
    assert_eq!(
        folder_name(&server, shared).await.as_deref(),
        Some("Shared")
    );
    assert_eq!(
        folder_name(&server, private).await.as_deref(),
        Some("Private")
    );

    let response = bob
        .form(Method::POST, &format!("/ui/folders/{family}"), rename)
        .await;
    response.assert_status(StatusCode::OK);
    assert_eq!(folder_name(&server, family).await.as_deref(), Some("Mine"));

    let response = alice
        .form(Method::DELETE, &format!("/ui/folders/{private}"), "")
        .await;
    let response = response.assert_status(StatusCode::OK);
    assert_eq!(response.header(hx("hx-redirect")), "/folders");
    assert_eq!(folder_name(&server, private).await, None);
}

#[sqlx::test]
async fn test_ui_writes_check_the_origin(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;
    let post = async |origin: Option<&str>| {
        let mut headers = vec![(header::HOST, "photos.example")];
        headers.extend(origin.map(|origin| (header::ORIGIN, origin)));
        let body = "name=Origin&folder-visibility=personal";
        alice
            .form_with(Method::POST, "/ui/folders", body, &headers)
            .await
            .status
    };

    assert_eq!(post(None).await, StatusCode::OK);
    assert_eq!(post(Some("https://photos.example")).await, StatusCode::OK);
    for origin in [
        "https://evil.example",
        "null",
        "https://photos.example.evil",
    ] {
        assert_eq!(post(Some(origin)).await, StatusCode::FORBIDDEN, "{origin}");
    }

    // Reads are never checked
    let headers = [(header::ORIGIN, "https://evil.example")];
    let response = alice.get_with("/ui/dialogs/folder-name", &headers).await;
    response.assert_status(StatusCode::OK);
}
