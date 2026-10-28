use super::{TestServer, ids};
use crate::model::photo::Photo;
use crate::repo::FavoritesRepo;
use crate::repo::tests::{Dataset, insert_photo, photo};
use axum::http::StatusCode;
use serde_json::Value;
use sqlx::SqlitePool;
use time::macros::datetime;

#[sqlx::test]
async fn test_neighbors(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;

    let mut photos = Vec::new();
    for day in 1..=9u8 {
        let name = if day == 5 {
            "clip.MP4".to_string()
        } else {
            format!("{day}.jpg")
        };
        let created_at = datetime!(2024-06-01 10:00 UTC).replace_day(day).unwrap();
        let photo = Photo {
            created_at,
            ..photo(Some("alice"), &name)
        };
        photos.push(insert_photo(server.pool(), photo).await);
    }
    // Newest first
    photos.reverse();
    let range =
        |range: std::ops::Range<usize>| photos[range].iter().map(|p| p.id).collect::<Vec<_>>();
    server
        .pool()
        .insert_favorite(photos[4].id, "alice")
        .await
        .unwrap();

    let neighbors = async |query: String| {
        alice
            .get(&format!("/api/context/neighbors?ctx=timeline:all&{query}"))
            .await
    };
    let json = neighbors(format!("photo_id={}&before=2&after=3", photos[4].id))
        .await
        .assert_status(StatusCode::OK)
        .json();
    assert_eq!(
        (&json["index"], &json["total"], &json["start"]),
        (&4.into(), &9.into(), &2.into())
    );
    assert_eq!(ids(&json["photos"]), range(2..8));
    let current = &json["photos"][2];
    assert_eq!(
        current,
        &serde_json::json!({
            "id": photos[4].id,
            "name": "clip.MP4",
            "created_at": photos[4].created_at.unix_timestamp(),
            "is_video": true,
            "thumb_hash": null,
            "file_size": 1024,
            "user_id": "alice",
            "folder_id": null,
            "is_favorite": true,
            "trashed_on": null,
            "deletable": true,
        })
    );
    assert_eq!(json["photos"][0]["is_favorite"], false);

    // Defaults (30/30) clamp to the context bounds
    let json = neighbors(format!("photo_id={}", photos[0].id)).await.json();
    assert_eq!((&json["index"], &json["start"]), (&0.into(), &0.into()));
    assert_eq!(ids(&json["photos"]), range(0..9));

    // Window mode: extending the loaded range without a photo
    let json = neighbors("start=7&limit=5".to_string()).await.json();
    assert_eq!((&json["index"], &json["start"]), (&Value::Null, &7.into()));
    assert_eq!(ids(&json["photos"]), range(7..9));

    for (uri, status) in [
        // Not in the context
        (
            format!(
                "/api/context/neighbors?ctx=timeline:family&photo_id={}",
                photos[0].id
            ),
            StatusCode::NOT_FOUND,
        ),
        (
            "/api/context/neighbors?ctx=timeline:nope&photo_id=1".to_string(),
            StatusCode::BAD_REQUEST,
        ),
        // Neither a photo nor a window
        (
            "/api/context/neighbors?ctx=timeline:all".to_string(),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        assert_eq!(alice.get(&uri).await.status, status, "{uri}");
    }
}

#[sqlx::test]
async fn test_ids(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;

    let june = insert_photo(server.pool(), photo(Some("alice"), "june.jpg")).await;
    let may = Photo {
        created_at: datetime!(2024-05-01 10:00 UTC),
        ..photo(None, "may.jpg")
    };
    let may = insert_photo(server.pool(), may).await;

    let json = alice.get("/api/context/ids?ctx=timeline:all").await.json();
    assert_eq!(ids(&json), [june.id, may.id]);
    let json = alice
        .get("/api/context/ids?ctx=timeline:all&month=2024-05")
        .await
        .json();
    assert_eq!(ids(&json), [may.id]);
    let response = alice
        .get("/api/context/ids?ctx=timeline:all&month=2024-5")
        .await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn test_folder_contexts(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let data = Dataset::create(server.pool()).await;
    let alice = server.login("alice").await;

    // Accessible folders: their photos and whether alice can delete them
    for (folder, name, deletable) in [
        (data.own_folder, "own_2", true),
        (data.shared_folder, "shared_1", false),
        (data.shared_delete_folder, "shared_delete_1", true),
    ] {
        let uri = format!(
            "/api/context/neighbors?ctx=folder:{folder}&photo_id={}",
            data.id(name)
        );
        let json = alice.get(&uri).await.assert_status(StatusCode::OK).json();
        assert_eq!(json["total"], 1, "{name}");
        assert_eq!(json["photos"][0]["deletable"], deletable, "{name}");
    }

    for folder in [data.expired_folder, data.unshared_folder] {
        let response = alice
            .get(&format!("/api/context/ids?ctx=folder:{folder}"))
            .await;
        assert_eq!(response.status, StatusCode::NOT_FOUND);
    }
}
