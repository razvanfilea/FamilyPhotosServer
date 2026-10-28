use super::TestServer;
use crate::repo::FolderPermissionsRepo;
use crate::repo::tests::{Dataset, create_test_folder};
use axum::http::StatusCode;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use time::OffsetDateTime;
use time::macros::datetime;

#[sqlx::test]
async fn test_upload_timestamps_and_names(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;

    // (query, file name, created_at): Android sends the time, else the file name is parsed,
    // then the fallback is used
    for (query, name, created_at) in [
        ("time_created=1700000000", "android.jpg", 1_700_000_000),
        (
            "",
            "IMG_20240102_030405.jpg",
            datetime!(2024-01-02 03:04:05 UTC).unix_timestamp(),
        ),
        ("fallback_time=1600000000", "no_date.jpg", 1_600_000_000),
    ] {
        let json = alice
            .upload(query, name, name.as_bytes())
            .await
            .assert_status(StatusCode::OK)
            .json();
        assert_eq!(json["created_at"], created_at, "{name}");
        assert_eq!(
            (&json["user_id"], &json["name"]),
            (&"alice".into(), &name.into())
        );
        assert_eq!(json["file_size"], name.len());
    }

    // Nothing at all: now
    let json = alice
        .upload("make_public=true", "now.png", b"now")
        .await
        .json();
    assert_eq!(json["user_id"], Value::Null);
    let now = OffsetDateTime::now_utc().unix_timestamp();
    assert!((json["created_at"].as_i64().unwrap() - now).abs() < 60);

    // Names that only need clean-up are sanitised
    let json = alice.upload("", "  spaced.jpg ", b"spaced").await.json();
    assert_eq!(json["name"], "spaced.jpg");
    let stored = server.state.storage.resolve_photo("alice/spaced.jpg");
    assert_eq!(std::fs::read(stored).unwrap(), b"spaced");
    let json = alice.upload("", ".hidden.jpg", b"hidden").await.json();
    let name = json["name"].as_str().unwrap();
    assert!(!name.starts_with('.') && name.ends_with(".jpg"), "{name}");

    // Names that aren't a single path component are rejected
    let count = || async {
        sqlx::query_scalar::<_, i64>("select count(*) from photos")
            .fetch_one(server.pool())
            .await
            .unwrap()
    };
    let before = count().await;
    for name in ["../../etc/passwd", "a/b.jpg", "a\\b.jpg", "..", "."] {
        let response = alice.upload("", name, b"evil").await;
        assert_eq!(
            response.assert_status(StatusCode::BAD_REQUEST).text(),
            "Invalid file name",
            "{name:?}"
        );
    }
    assert_eq!(count().await, before);
    assert!(!server.state.storage.resolve_photo("alice/passwd").exists());

    // Names that are only unsafe after trimming, or hidden, pass the check and are renamed
    for name in ["..hidden", " .. ", "   "] {
        let json = alice
            .upload("", name, name.as_bytes())
            .await
            .assert_status(StatusCode::OK)
            .json();
        let stored = json["name"].as_str().unwrap();
        assert!(
            !stored.starts_with('.') && !stored.contains(['/', '\\']) && !stored.trim().is_empty(),
            "{name:?} → {stored:?}"
        );
    }
    let before = count().await;
    // A NUL can't even be sent: multipart parsing rejects it before the handler sees the name
    let response = alice.upload("", "a\0b.jpg", b"evil").await;
    assert!(!response.status.is_success());
    assert_eq!(count().await, before);

    // A different file with an existing name gets a new name
    let json = alice.upload("", "spaced.jpg", b"other").await.json();
    assert_ne!(json["name"], "spaced.jpg");
}

#[sqlx::test]
async fn test_upload_dedupe_and_destinations(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;
    let bob = server.login("bob").await;
    let folder = create_test_folder(server.pool(), Some("alice"), "shared").await;
    let view_only = create_test_folder(server.pool(), Some("alice"), "view-only").await;
    server
        .pool()
        .create_share(folder.id, Some("bob"), true, false, None)
        .await
        .unwrap();
    server
        .pool()
        .create_share(view_only.id, Some("bob"), false, false, None)
        .await
        .unwrap();

    let content = b"some photo bytes";
    let full = blake3::hash(content).to_hex().to_string();
    let cropped = full[..32].to_string();
    let first = alice.upload("", "photo.jpg", content).await.json();

    // The same content for the same owner returns the existing photo, with or without the hash
    for query in [
        String::new(),
        format!("hash={full}"),
        format!("hash={cropped}"),
    ] {
        let json = alice.upload(&query, "copy.jpg", content).await.json();
        assert_eq!(json, first, "{query}");
    }
    // Other owners get their own copy
    let public = alice
        .upload("make_public=true", "photo.jpg", content)
        .await
        .json();
    assert_ne!(public["id"], first["id"]);
    assert_eq!(public["user_id"], Value::Null);

    // Into a shared folder: the photo belongs to the folder owner
    let uri = format!("folder_id={}", folder.id);
    let bobs = bob.upload(&uri, "bobs.jpg", b"new content").await.json();
    assert_eq!(
        (&bobs["user_id"], &bobs["folder_id"]),
        (&"alice".into(), &folder.id.into())
    );
    assert_eq!(
        bob.upload(&uri, "again.jpg", b"new content").await.json(),
        bobs
    );
    // ...but is only deduplicated against that folder, so bob can't probe alice's other photos
    let copy = bob.upload(&uri, "copy.jpg", content).await.json();
    assert_ne!(copy["id"], first["id"]);
    assert_eq!(
        (&copy["user_id"], &copy["folder_id"]),
        (&"alice".into(), &folder.id.into())
    );
    assert_eq!(bob.upload(&uri, "copy2.jpg", content).await.json(), copy);
    // The owner's uploads into the folder still match all of their photos
    assert_eq!(alice.upload(&uri, "copy3.jpg", content).await.json(), first);

    // No upload permission: 401, as Android expects
    let unknown = create_test_folder(server.pool(), Some("carol"), "private").await;
    for folder_id in [view_only.id, unknown.id] {
        let response = bob
            .upload(&format!("folder_id={folder_id}"), "x.jpg", b"x")
            .await;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED, "{folder_id}");
    }
}

#[sqlx::test]
async fn test_check_hashes(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;
    let bob = server.login("bob").await;
    let check = async |client: &super::Client<'_>, body| {
        client.post("/api/upload/check-hashes", body).await
    };

    let content = b"some photo bytes";
    alice
        .upload("", "photo.jpg", content)
        .await
        .assert_status(StatusCode::OK);
    let full = blake3::hash(content).to_hex().to_string();
    let cropped = full[..32].to_string();
    let unknown = blake3::hash(b"other").to_hex().to_string();

    // The matching hashes, as sent
    let json = check(&alice, json!({"hashes": [full, cropped, unknown]}))
        .await
        .json();
    assert_eq!(json, json!({"existing": [full, cropped]}));

    // Other destinations don't have it
    let json = check(&alice, json!({"hashes": [full], "make_public": true}))
        .await
        .json();
    assert_eq!(json, json!({"existing": []}));
    let json = check(&bob, json!({"hashes": [full]})).await.json();
    assert_eq!(json, json!({"existing": []}));

    // Into a folder: the folder owner's photos count
    let folder = create_test_folder(server.pool(), Some("alice"), "shared").await;
    server
        .pool()
        .create_share(folder.id, Some("bob"), true, false, None)
        .await
        .unwrap();
    let json = check(&alice, json!({"hashes": [full], "folder_id": folder.id}))
        .await
        .json();
    assert_eq!(json, json!({"existing": [full]}));
    // ...but for a grantee only those in the shared folder, so bob can't probe alice's photos
    let json = check(&bob, json!({"hashes": [full], "folder_id": folder.id}))
        .await
        .json();
    assert_eq!(json, json!({"existing": []}));
    let in_folder = b"shared photo bytes";
    alice
        .upload(&format!("folder_id={}", folder.id), "in.jpg", in_folder)
        .await
        .assert_status(StatusCode::OK);
    let in_folder = blake3::hash(in_folder).to_hex().to_string();
    let body = json!({"hashes": [full, in_folder], "folder_id": folder.id});
    let json = check(&bob, body).await.json();
    assert_eq!(json, json!({"existing": [in_folder]}));

    let private = create_test_folder(server.pool(), Some("carol"), "private").await;
    let too_many = vec![full.clone(); 1001];
    for (client, body, status) in [
        // Unlike the upload, 404 for inaccessible folders
        (
            &bob,
            json!({"hashes": [full], "folder_id": private.id}),
            StatusCode::NOT_FOUND,
        ),
        (
            &alice,
            json!({"hashes": ["nothex"]}),
            StatusCode::BAD_REQUEST,
        ),
        (&alice, json!({"hashes": too_many}), StatusCode::BAD_REQUEST),
    ] {
        assert_eq!(check(client, body).await.status, status);
    }
}

/// Which folders alice may upload to
#[sqlx::test]
async fn test_upload_folder_permissions(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let data = Dataset::create(server.pool()).await;
    let alice = server.login("alice").await;
    let public = create_test_folder(server.pool(), None, "family").await;

    // Own and public folders are open, shared ones need `can_upload` and a live share
    let cases = [
        (data.own_folder, "alice", StatusCode::OK),
        (public.id, "", StatusCode::OK),
        (data.shared_delete_folder, "bob", StatusCode::OK),
        (data.shared_folder, "", StatusCode::UNAUTHORIZED),
        (data.expired_folder, "", StatusCode::UNAUTHORIZED),
        (data.unshared_folder, "", StatusCode::UNAUTHORIZED),
    ];
    for (folder_id, owner, status) in cases {
        let content = format!("content of {folder_id}");
        let response = alice
            .upload(
                &format!("folder_id={folder_id}"),
                "new.jpg",
                content.as_bytes(),
            )
            .await;
        assert_eq!(response.status, status, "{folder_id}");
        if status == StatusCode::OK {
            let json = response.json();
            assert_eq!(json["folder_id"], folder_id);
            let owner = if owner.is_empty() {
                Value::Null
            } else {
                owner.into()
            };
            assert_eq!(json["user_id"], owner, "{folder_id}");
        }
    }
}
