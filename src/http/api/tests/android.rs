//! The `/api` contract the Android app relies on: authentication, profile and sync shapes.

use super::{TestServer, ids};
use crate::repo::tests::{Dataset, insert_photo, photo};
use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::SqlitePool;

#[sqlx::test]
async fn test_protected_routes_answer_401(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let anonymous = server.anonymous();

    let routes = [
        (Method::GET, "/api/profile"),
        (Method::GET, "/api/members"),
        (Method::GET, "/api/sync/full"),
        (Method::GET, "/api/sync/partial?last_synced_event_id=0"),
        (Method::POST, "/api/sync/folders"),
        (Method::GET, "/api/context/ids?ctx=timeline:all"),
        (
            Method::GET,
            "/api/context/neighbors?ctx=timeline:all&start=0",
        ),
        (Method::POST, "/api/move"),
        (Method::POST, "/api/trash"),
        (Method::POST, "/api/trash/restore"),
        (Method::GET, "/api/sharing"),
        (Method::POST, "/api/sharing"),
        (Method::PUT, "/api/sharing/1"),
        (Method::DELETE, "/api/sharing/1"),
        (Method::GET, "/api/sharing/folder/1"),
        (Method::GET, "/api/favorite"),
        (Method::POST, "/api/favorite/1"),
        (Method::DELETE, "/api/favorite/1"),
        (Method::POST, "/api/folders"),
        (Method::PATCH, "/api/folders/1"),
        (Method::POST, "/api/upload"),
        (Method::POST, "/api/upload/check-hashes"),
        (Method::GET, "/api/duplicates"),
        (Method::POST, "/api/timestamp/1"),
        (Method::DELETE, "/api/delete/1"),
        (Method::GET, "/api/exif/1"),
        (Method::GET, "/api/photos/1/info"),
        (Method::GET, "/api/download/1"),
        (Method::GET, "/api/preview/1"),
        (Method::GET, "/api/screen/1"),
    ];
    for (method, uri) in routes {
        let response = anonymous.request(method.clone(), uri, &[], None).await;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED, "{method} {uri}");
    }
}

#[sqlx::test]
async fn test_session(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;

    let profile = alice
        .get("/api/profile")
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(
        profile.json(),
        json!({"user_id": "alice", "display_name": "Alice"})
    );
    let members = alice
        .get("/api/members")
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(
        members.json(),
        json!([
            {"user_id": "alice", "display_name": "Alice"},
            {"user_id": "bob", "display_name": "Bob"},
            {"user_id": "carol", "display_name": "Carol"},
        ])
    );

    let response = alice.request(Method::POST, "/api/logout", &[], None).await;
    assert_eq!(
        response.assert_status(StatusCode::OK).json(),
        json!({"message": "Logged out"})
    );
    assert_eq!(
        alice.get("/api/profile").await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test]
async fn test_full_and_partial_sync(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;

    // An empty log: the cursor is 0
    let json = alice
        .get("/api/sync/full")
        .await
        .assert_status(StatusCode::OK)
        .json();
    assert_eq!(json, json!({"event_log_id": 0, "photos": []}));

    let data = Dataset::create(server.pool()).await;
    let json = alice
        .get("/api/sync/full")
        .await
        .assert_status(StatusCode::OK)
        .json();
    let cursor = json["event_log_id"].as_i64().unwrap();
    // Own and public photos, trashed ones included
    let mut names = data.names(ids(&json["photos"]));
    names.sort();
    assert_eq!(
        names,
        [
            "own_1",
            "own_2",
            "own_big",
            "own_old",
            "own_trashed",
            "public_a",
            "public_b",
            "public_big",
            "public_trashed"
        ]
    );

    // Delta events: own and public photos only, with the photo as base64 JSON, null for deletes
    let mine = insert_photo(server.pool(), photo(Some("alice"), "new.jpg")).await;
    insert_photo(server.pool(), photo(Some("bob"), "bobs.jpg")).await;
    let uri = |cursor: i64| format!("/api/sync/partial?last_synced_event_id={cursor}");
    let json = alice
        .get(&uri(cursor))
        .await
        .assert_status(StatusCode::OK)
        .json();
    assert_eq!(json["event_log_id"], cursor + 2);
    assert_eq!(json["events"].as_array().unwrap().len(), 1);
    assert_eq!(json["events"][0]["photo_id"], mine.id);
    assert!(json["events"][0]["data"].is_string());

    let up_to_date = alice
        .get(&uri(cursor + 2))
        .await
        .assert_status(StatusCode::OK)
        .json();
    assert_eq!(
        up_to_date,
        json!({"event_log_id": cursor + 2, "events": []})
    );

    // A cursor outside the log means a full sync is needed
    for stale in [cursor + 100, -5] {
        let response = alice.get(&uri(stale)).await;
        assert_eq!(response.status, StatusCode::CONFLICT, "{stale}");
    }
    assert_eq!(
        alice.get("/api/sync/partial").await.status,
        StatusCode::BAD_REQUEST,
        "the cursor is required"
    );
}

#[sqlx::test]
async fn test_folder_sync(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let data = Dataset::create(server.pool()).await;
    let alice = server.login("alice").await;
    let sync = async |cursors: Value| {
        let response = alice.post("/api/sync/folders", cursors).await;
        response.assert_status(StatusCode::OK).json()
    };
    let find = |json: &Value, id: i64| {
        json.as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"] == id)
            .cloned()
            .unwrap_or(Value::Null)
    };

    // Without cursors: the accessible folders, no photos. Expired and unshared ones are hidden.
    let json = sync(json!({})).await;
    let mut folder_ids = ids(&json);
    folder_ids.sort();
    let mut expected = vec![
        data.own_folder,
        data.shared_folder,
        data.shared_delete_folder,
    ];
    expected.sort();
    assert_eq!(folder_ids, expected);
    assert_eq!(
        find(&json, data.own_folder),
        json!({"id": data.own_folder, "owner_id": "alice", "name": "mine"})
    );
    assert_eq!(
        find(&json, data.shared_folder),
        json!({"id": data.shared_folder, "owner_id": "bob", "name": "shared",
               "can_upload": false, "can_delete": false})
    );

    // Cursor 0: the full photo list and the cursor to continue from. Only shared folders get one.
    let cursors = json!({
        data.own_folder.to_string(): 0,
        data.shared_folder.to_string(): 0,
        data.shared_delete_folder.to_string(): 0,
    });
    let json = sync(cursors).await;
    let own = find(&json, data.own_folder);
    assert_eq!(
        (&own["photos"], &own["events"], &own["latest_event_id"]),
        (&Value::Null, &Value::Null, &Value::Null)
    );
    let shared = find(&json, data.shared_folder);
    assert_eq!(shared["events"], Value::Null);
    // Trashed photos are not part of it
    assert_eq!(data.names(ids(&shared["photos"])), ["shared_1"]);
    let latest = shared["latest_event_id"].as_i64().unwrap();
    assert!(latest > 0);
    let delete = find(&json, data.shared_delete_folder);
    assert_eq!(
        (delete["can_upload"].clone(), delete["can_delete"].clone()),
        (true.into(), true.into())
    );

    // A valid cursor: only the events after it
    let cursor_json = |cursor: i64| json!({data.shared_folder.to_string(): cursor});
    let json = sync(cursor_json(latest)).await;
    let shared = find(&json, data.shared_folder);
    assert_eq!(shared["events"], json!([]));
    assert_eq!(shared["photos"], Value::Null);
    assert_eq!(shared["latest_event_id"], latest);

    let new = insert_photo(
        server.pool(),
        crate::model::photo::Photo {
            folder_id: Some(data.shared_folder),
            ..photo(Some("bob"), "new.jpg")
        },
    )
    .await;
    let json = sync(cursor_json(latest)).await;
    let shared = find(&json, data.shared_folder);
    assert_eq!(shared["events"].as_array().unwrap().len(), 1);
    assert_eq!(shared["events"][0]["photo_id"], new.id);
    assert!(shared["events"][0]["data"].is_string());
    assert!(shared["latest_event_id"].as_i64().unwrap() > latest);

    // A cursor ahead of the log is stale: the full list again
    let json = sync(cursor_json(latest + 10_000)).await;
    let shared = find(&json, data.shared_folder);
    assert_eq!(shared["events"], Value::Null);
    assert_eq!(ids(&shared["photos"]).len(), 2);
}
