use super::{TestServer, ids};
use crate::repo::FolderPermissionsRepo;
use crate::repo::tests::create_test_folder;
use axum::http::StatusCode;
use serde_json::json;
use sqlx::SqlitePool;

#[sqlx::test]
async fn test_folder_validation_and_conflicts(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;
    let create = |name: &str| alice.post("/api/folders", json!({"name": name, "is_public": false}));

    // Nothing that could escape the user's directory (or hide in it)
    let invalid = [
        "",
        "  ",
        ".",
        "..",
        ".previews",
        "a/b",
        "a\\b",
        "../bob",
        "/tmp/x",
        "a\0b",
    ];
    for name in invalid {
        assert_eq!(
            create(name).await.status,
            StatusCode::BAD_REQUEST,
            "{name:?}"
        );
    }
    assert_eq!(
        create("a/b").await.text(),
        "Folder names can't contain / or \\"
    );

    let vacation = create(" Vacation ")
        .await
        .assert_status(StatusCode::OK)
        .json();
    assert_eq!(vacation["name"], "Vacation");
    // Upsert: an existing name returns the existing folder
    assert_eq!(create("Vacation").await.json()["id"], vacation["id"]);

    let work = create("Work").await.json();
    let rename = async |name: &str| {
        let uri = format!("/api/folders/{}", work["id"]);
        alice.patch(&uri, json!({"name": name})).await
    };
    // Neither folder has a directory yet: renaming only touches the DB
    let response = rename("Vacation").await;
    assert_eq!(response.status, StatusCode::CONFLICT);
    assert_eq!(response.text(), "A folder with this name already exists");
    for name in invalid.into_iter().filter(|name| !name.trim().is_empty()) {
        assert_eq!(
            rename(name).await.status,
            StatusCode::BAD_REQUEST,
            "{name:?}"
        );
    }
    let renamed = rename("Work 2024")
        .await
        .assert_status(StatusCode::OK)
        .json();
    assert_eq!(renamed["name"], "Work 2024");

    // A directory with the target name exists on disk
    let taken = server
        .state
        .storage
        .resolve_folder(Some("alice"), Some("Taken"));
    std::fs::create_dir_all(&taken).unwrap();
    assert_eq!(rename("Taken").await.status, StatusCode::CONFLICT);
}

/// Public folders are visible to everyone, so making a folder public drops its per-user shares
#[sqlx::test]
async fn test_make_public_drops_user_shares(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;
    let mine = create_test_folder(server.pool(), Some("alice"), "mine").await;
    let other = create_test_folder(server.pool(), Some("alice"), "other").await;
    for (folder_id, grantee) in [
        (mine.id, Some("bob")),
        (mine.id, None),
        (other.id, Some("bob")),
    ] {
        server
            .pool()
            .create_share(folder_id, grantee, true, false, None)
            .await
            .unwrap();
    }
    let shares = async |folder_id: i64| {
        let json = alice
            .get(&format!("/api/sharing/folder/{folder_id}"))
            .await
            .assert_status(StatusCode::OK)
            .json();
        let mut grantees: Vec<_> = json
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["grantee_id"].clone())
            .collect();
        grantees.sort_by_key(|g| g.to_string());
        grantees
    };
    let bob = json!("bob");
    assert_eq!(shares(mine.id).await, [bob.clone(), json!(null)]);

    // Renaming keeps the shares
    let uri = format!("/api/folders/{}", mine.id);
    alice
        .patch(&uri, json!({"name": "renamed"}))
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(shares(mine.id).await, [bob.clone(), json!(null)]);

    // Only the link share is left, and other folders keep theirs
    alice
        .patch(&uri, json!({"is_public": true}))
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(shares(mine.id).await, [json!(null)]);
    assert_eq!(shares(other.id).await, [bob]);
}

#[sqlx::test]
async fn test_sharing(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;
    let mine = create_test_folder(server.pool(), Some("alice"), "mine").await;
    let bobs = create_test_folder(server.pool(), Some("bob"), "bobs").await;
    let public = create_test_folder(server.pool(), None, "family").await;
    server
        .pool()
        .create_share(bobs.id, Some("alice"), true, true, None)
        .await
        .unwrap();

    let share = |folder_id: i64| {
        alice.post(
            "/api/sharing",
            json!({"folder_id": folder_id, "grantee_id": "bob"}),
        )
    };
    assert_eq!(share(mine.id).await.status, StatusCode::OK);
    let response = share(mine.id).await;
    assert_eq!(response.status, StatusCode::CONFLICT);
    assert_eq!(response.text(), "This person already has access");

    // Only the owner shares, even with full access; public folders are already shared
    assert_eq!(share(bobs.id).await.status, StatusCode::NOT_FOUND);
    assert_eq!(share(public.id).await.status, StatusCode::BAD_REQUEST);
}

/// Listing, updating and revoking shares: owners, other users and public folders
#[sqlx::test]
async fn test_share_management(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;
    let bob = server.login("bob").await;
    let mine = create_test_folder(server.pool(), Some("alice"), "mine").await;
    let family = create_test_folder(server.pool(), None, "family").await;
    let carols = create_test_folder(server.pool(), Some("carol"), "carols").await;
    let create = async |folder_id: i64, grantee: &str| {
        let share = server
            .pool()
            .create_share(folder_id, Some(grantee), true, false, None)
            .await
            .unwrap();
        share.id
    };
    let to_bob = create(mine.id, "bob").await;
    let to_carol = create(mine.id, "carol").await;
    let public = create(family.id, "carol").await;
    let carols_share = create(carols.id, "alice").await;

    // Owners see the shares of their folders, everybody the ones of public folders
    let json = alice
        .get("/api/sharing")
        .await
        .assert_status(StatusCode::OK)
        .json();
    let mut listed = ids(&json);
    listed.sort();
    assert_eq!(listed, [to_bob, to_carol, public]);
    assert!(json[0]["folder_name"].is_string());
    assert_eq!(ids(&bob.get("/api/sharing").await.json()), [public]);

    // The shares of a folder, for anybody who can access it (grantees included)
    let uri = format!("/api/sharing/folder/{}", mine.id);
    for client in [&alice, &bob] {
        let json = client.get(&uri).await.assert_status(StatusCode::OK).json();
        assert_eq!(ids(&json).len(), 2);
        assert_eq!(json[0]["folder_name"], "mine");
    }
    let uri = format!("/api/sharing/folder/{}", family.id);
    assert_eq!(ids(&bob.get(&uri).await.json()), [public]);
    let uri = format!("/api/sharing/folder/{}", carols.id);
    assert_eq!(alice.get(&uri).await.status, StatusCode::OK);
    assert_eq!(bob.get(&uri).await.status, StatusCode::NOT_FOUND);

    // Only the owner changes or revokes a share of a private folder
    let update = json!({"can_upload": false, "can_delete": true});
    for (client, share, status) in [
        (&bob, to_bob, StatusCode::NOT_FOUND),
        (&bob, carols_share, StatusCode::NOT_FOUND),
        (&alice, carols_share, StatusCode::NOT_FOUND),
        (&alice, 9999, StatusCode::NOT_FOUND),
        (&alice, to_bob, StatusCode::OK),
        // Anybody manages the shares of a public folder
        (&bob, public, StatusCode::OK),
    ] {
        let response = client
            .put(&format!("/api/sharing/{share}"), update.clone())
            .await;
        assert_eq!(response.status, status, "update {share}");
        if status == StatusCode::OK {
            let json = response.json();
            assert_eq!(
                (&json["can_upload"], &json["can_delete"]),
                (&false.into(), &true.into())
            );
            assert_eq!(json["id"], share);
        }
    }
    let json = alice
        .get(&format!("/api/sharing/folder/{}", mine.id))
        .await
        .json();
    let updated = json
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == to_bob)
        .unwrap();
    assert_eq!(
        (&updated["can_upload"], &updated["can_delete"]),
        (&false.into(), &true.into())
    );
    let untouched = json
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == to_carol)
        .unwrap();
    assert_eq!(untouched["can_upload"], true);

    for (client, share, status) in [
        (&bob, to_carol, StatusCode::NOT_FOUND),
        (&alice, 9999, StatusCode::NOT_FOUND),
        (&alice, to_carol, StatusCode::OK),
        (&alice, to_carol, StatusCode::NOT_FOUND),
        (&bob, public, StatusCode::OK),
    ] {
        let response = client.delete(&format!("/api/sharing/{share}")).await;
        assert_eq!(response.status, status, "revoke {share}");
    }
    assert_eq!(ids(&alice.get("/api/sharing").await.json()), [to_bob]);
}
