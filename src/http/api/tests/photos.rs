use super::{TestServer, ids};
use crate::repo::tests::{Dataset, insert_photo, photo};
use crate::repo::{FavoritesRepo, PhotosRepo};
use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use time::OffsetDateTime;

/// What alice may do with each photo of the dataset, through the API
#[sqlx::test]
async fn test_permission_matrix(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let data = Dataset::create(server.pool()).await;
    let alice = server.login("alice").await;
    const OK: StatusCode = StatusCode::OK;
    const NOT_FOUND: StatusCode = StatusCode::NOT_FOUND;

    // (photo, info and favorite (read access), trash and restore (delete access))
    let cases = [
        ("own_1", OK, OK),
        ("own_2", OK, OK),
        ("own_trashed", OK, OK),
        ("public_a", OK, OK),
        ("shared_1", OK, NOT_FOUND),
        ("shared_trashed", OK, NOT_FOUND),
        ("shared_delete_1", OK, OK),
        ("expired_1", NOT_FOUND, NOT_FOUND),
        ("unshared_1", NOT_FOUND, NOT_FOUND),
        ("carol_root", NOT_FOUND, NOT_FOUND),
    ];
    for (name, read, delete) in cases {
        let id = data.id(name);
        let info = alice.get(&format!("/api/photos/{id}/info")).await;
        assert_eq!(info.status, read, "{name}");
        if read == OK {
            let json = info.json();
            assert_eq!(json["can_delete"], delete == OK, "{name}");
            assert_eq!(json["can_move"], delete == OK, "{name}");
        }

        let favorite_uri = format!("/api/favorite/{id}");
        let favorite = alice.request(Method::POST, &favorite_uri, &[], None);
        assert_eq!(favorite.await.status, read, "favorite {name}");

        let trashed = alice.post("/api/trash", json!([id])).await;
        assert_eq!(trashed.status, delete, "trash {name}");
        let restored = alice.post("/api/trash/restore", json!([id])).await;
        assert_eq!(restored.status, delete, "restore {name}");
        if delete == OK {
            assert!(trashed.json()[0]["trashed_on"].is_i64(), "{name}");
            assert_eq!(restored.json()[0]["trashed_on"], Value::Null, "{name}");
        }
    }

    // A live photo: trashing stamps the current time, restoring clears it
    let fresh = insert_photo(server.pool(), photo(Some("alice"), "fresh.jpg")).await;
    let trashed = alice.post("/api/trash", json!([fresh.id])).await.json();
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let trashed_on = trashed[0]["trashed_on"].as_i64().unwrap();
    assert!((trashed_on - now).abs() < 60, "{trashed_on}");
    let stored = server.pool().get_photo(fresh.id).await.unwrap().unwrap();
    assert_eq!(
        stored.trashed_on.map(|t| t.unix_timestamp()),
        Some(trashed_on)
    );
    let restored = alice
        .post("/api/trash/restore", json!([fresh.id]))
        .await
        .json();
    assert_eq!(restored[0]["trashed_on"], Value::Null);
    let stored = server.pool().get_photo(fresh.id).await.unwrap().unwrap();
    assert_eq!(stored.trashed_on, None);

    // One inaccessible photo fails the whole request
    let ids = json!([data.id("own_1"), data.id("shared_1")]);
    let response = alice.post("/api/trash", ids).await;
    assert_eq!(response.status, NOT_FOUND);
    let own_1 = alice
        .get(&format!("/api/photos/{}/info", data.id("own_1")))
        .await;
    assert_eq!(own_1.json()["photo"]["trashed_on"], Value::Null);
}

#[sqlx::test]
async fn test_photo_info(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let data = Dataset::create(server.pool()).await;
    let alice = server.login("alice").await;
    let public = insert_photo(server.pool(), photo(None, "clip.mov")).await;
    server
        .pool()
        .insert_favorite(public.id, "alice")
        .await
        .unwrap();

    let json = alice
        .get(&format!("/api/photos/{}/info", data.id("shared_1")))
        .await
        .json();
    assert_eq!(json["photo"]["id"], data.id("shared_1"));
    assert_eq!(
        json["folder"],
        json!({"id": data.shared_folder, "name": "shared"})
    );
    assert_eq!(
        json["owner"],
        json!({"user_id": "bob", "display_name": "Bob"})
    );
    assert_eq!(json["is_favorite"], false);
    // The file doesn't exist: only the label is known
    assert_eq!(json["details"]["media_label"], "Image");
    assert_eq!(json["details"]["width"], Value::Null);

    let json = alice
        .get(&format!("/api/photos/{}/info", public.id))
        .await
        .json();
    assert_eq!(
        (&json["folder"], &json["owner"]),
        (&Value::Null, &Value::Null)
    );
    assert_eq!(json["is_favorite"], true);
    assert_eq!(json["details"]["media_label"], "Video (MOV)");
}

#[sqlx::test]
async fn test_favorites(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;
    let photo = insert_photo(server.pool(), photo(Some("alice"), "a.jpg")).await;
    let uri = format!("/api/favorite/{}", photo.id);

    // Idempotent
    for _ in 0..2 {
        let response = alice.request(Method::POST, &uri, &[], None).await;
        assert_eq!(response.status, StatusCode::OK);
    }
    assert_eq!(alice.get("/api/favorite").await.json(), json!([photo.id]));

    let response = alice.request(Method::DELETE, &uri, &[], None).await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(alice.get("/api/favorite").await.json(), json!([]));
}

/// Moves between folders, through `POST /api/move`: who may move what, and the folder event log
#[sqlx::test]
async fn test_move_photos(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let data = Dataset::create(server.pool()).await;
    let alice = server.login("alice").await;
    let (mine, shared_delete) = (data.own_folder, data.shared_delete_folder);

    // The folder events of the photo logged after `after`: `(folder, whether it was removed)`
    let events = async |photo_id: i64, after: i64| {
        sqlx::query_as::<_, (i64, bool)>(
            "select folder_id, data is null from folder_event_log
            where photo_id = ? and event_id > ? order by event_id",
        )
        .bind(photo_id)
        .bind(after)
        .fetch_all(server.pool())
        .await
        .unwrap()
    };

    // (photo, folder name, query, moved to (owner, folder), folder events)
    #[rustfmt::skip]
    let cases = [
        // Into one of alice's folders
        ("own_1", None, format!("target_folder_id={mine}"), Some(("alice", Some(mine))), vec![(mine, false)]),
        // Out of a folder, to the root of alice
        ("own_2", Some("mine"), String::new(), Some(("alice", None)), vec![(mine, true)]),
        // To the public root
        ("own_old", None, "make_public=true".to_string(), Some(("", None)), vec![]),
        // With delete access to the source, the photo becomes the target owner's
        ("shared_delete_1", Some("shared-delete"), format!("target_folder_id={mine}"), Some(("alice", Some(mine))),
            vec![(shared_delete, true), (mine, false)]),
        // View-only: skipped without an error
        ("shared_1", Some("shared"), format!("target_folder_id={mine}"), None, vec![]),
    ];
    for (name, folder_name, query, moved, expected_events) in cases {
        let id = data.id(name);
        let photo = server.pool().get_photo(id).await.unwrap().unwrap();
        server.write_file(&photo, folder_name, name.as_bytes());
        let after: i64 =
            sqlx::query_scalar("select coalesce(max(event_id), 0) from folder_event_log")
                .fetch_one(server.pool())
                .await
                .unwrap();

        let response = alice.post(&format!("/api/move?{query}"), json!([id])).await;
        let json = response.assert_status(StatusCode::OK).json();
        let stored = server.pool().get_photo(id).await.unwrap().unwrap();
        match moved {
            Some((owner, folder_id)) => {
                assert_eq!(ids(&json), [id], "{name}");
                let owner = (!owner.is_empty()).then(|| owner.to_string());
                assert_eq!(
                    (stored.user_id, stored.folder_id),
                    (owner, folder_id),
                    "{name}"
                );
            }
            None => {
                assert_eq!(json, json!([]), "{name}");
                assert_eq!(stored, photo, "{name}");
            }
        }
        assert_eq!(events(id, after).await, expected_events, "{name}");
    }

    // The destination must accept uploads
    for folder in [
        data.shared_folder,
        data.expired_folder,
        data.unshared_folder,
    ] {
        let uri = format!("/api/move?target_folder_id={folder}");
        let response = alice.post(&uri, json!([data.id("own_big")])).await;
        assert_eq!(response.status, StatusCode::NOT_FOUND, "{folder}");
    }
}
