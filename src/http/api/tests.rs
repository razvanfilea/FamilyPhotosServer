//! Handler-level tests: the whole router (auth included) driven with `oneshot`.

use crate::db::SessionStore;
use crate::http::{AppState, AppStateRef, router};
use crate::model::photo::Photo;
use crate::model::user::User;
use crate::repo::tests::{create_test_folder, create_test_photo_with_time};
use crate::repo::{FavoritesRepo, FolderPermissionsRepo, PhotosTransactionRepo};
use crate::utils::password_hash::generate_hash_from_password;
use crate::utils::storage_resolver::StorageResolver;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::LazyLock;
use tempfile::TempDir;
use time::macros::datetime;
use tower::ServiceExt;

const PASSWORD: &str = "password";

/// Hashing is slow in debug builds, so hash once
static PASSWORD_HASH: LazyLock<String> = LazyLock::new(|| generate_hash_from_password(PASSWORD));

struct TestServer {
    router: Router,
    state: AppStateRef,
    _storage: TempDir,
}

struct TestResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl TestResponse {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| {
            panic!("{e}: {}", String::from_utf8_lossy(&self.body));
        })
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    fn header(&self, name: header::HeaderName) -> &str {
        self.headers
            .get(&name)
            .unwrap_or_else(|| panic!("missing {name}"))
            .to_str()
            .unwrap()
    }
}

impl TestServer {
    async fn new(pool: SqlitePool) -> Self {
        let storage_dir = TempDir::new().unwrap();
        let storage = StorageResolver::new(
            storage_dir.path().join("storage"),
            storage_dir.path().join("previews"),
        );

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
            _storage: storage_dir,
        }
    }

    fn pool(&self) -> &SqlitePool {
        &self.state.write_pool
    }

    async fn send(&self, request: Request<Body>) -> TestResponse {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        TestResponse {
            status,
            headers,
            body,
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
        assert_eq!(response.status, StatusCode::OK, "{}", response.text());
        response
            .header(header::SET_COOKIE)
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
    ) -> TestResponse {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::COOKIE, cookie);
        let body = match json_body {
            Some(body) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        self.send(builder.body(body).unwrap()).await
    }

    async fn get(&self, cookie: &str, uri: &str) -> TestResponse {
        self.request(cookie, Method::GET, uri, None).await
    }

    async fn insert_photo(&self, photo: Photo) -> Photo {
        let mut tx = self.pool().begin().await.unwrap();
        let photo = tx.insert_photo(&photo).await.unwrap();
        tx.commit().await.unwrap();
        photo
    }

    /// Writes the photo's file to the storage
    fn write_file(&self, photo: &Photo, folder_name: Option<&str>, content: &[u8]) -> PathBuf {
        let path = self
            .state
            .storage
            .resolve_photo(photo.partial_path(folder_name));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path
    }

    async fn check_hashes(&self, cookie: &str, body: Value) -> TestResponse {
        self.request(cookie, Method::POST, "/api/upload/check-hashes", Some(body))
            .await
    }

    async fn upload(
        &self,
        cookie: &str,
        query: &str,
        file_name: &str,
        content: &[u8],
    ) -> TestResponse {
        const BOUNDARY: &str = "test-boundary-1234";
        let mut body = format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .into_bytes();
        body.extend_from_slice(content);
        body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());

        self.send(
            Request::post(format!("/api/upload?{query}"))
                .header(header::COOKIE, cookie)
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={BOUNDARY}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
    }
}

fn ids(value: &Value) -> Vec<i64> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_i64().or_else(|| v.as_i64()).unwrap())
        .collect()
}

#[sqlx::test]
async fn test_new_endpoints_require_login(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    for uri in [
        "/api/context/neighbors?ctx=timeline:all&photo_id=1",
        "/api/context/ids?ctx=timeline:all",
        "/api/photos/1/info",
        "/api/screen/1",
    ] {
        let response = server.get("", uri).await;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED, "{uri}");
    }
}

#[sqlx::test]
async fn test_context_neighbors(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let cookie = server.login("alice").await;

    let mut photos = Vec::new();
    for day in 1..=9u8 {
        let created_at = datetime!(2024-06-01 10:00:00 UTC).replace_day(day).unwrap();
        let name = if day == 5 {
            "clip.MP4".to_string()
        } else {
            format!("{day}.jpg")
        };
        photos.push(
            server
                .insert_photo(create_test_photo_with_time(
                    0,
                    Some("alice"),
                    None,
                    &name,
                    created_at,
                ))
                .await,
        );
    }
    // Newest first
    photos.reverse();
    server
        .pool()
        .insert_favorite(photos[4].id, "alice")
        .await
        .unwrap();

    let response = server
        .get(
            &cookie,
            &format!(
                "/api/context/neighbors?ctx=timeline:all&photo_id={}&before=2&after=3",
                photos[4].id
            ),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text());
    let json = response.json();
    assert_eq!(json["index"], 4);
    assert_eq!(json["total"], 9);
    assert_eq!(json["start"], 2);
    assert_eq!(
        ids(&json["photos"]),
        photos[2..8].iter().map(|p| p.id).collect::<Vec<_>>()
    );
    let current = &json["photos"][2];
    assert_eq!(current["is_favorite"], true);
    assert_eq!(current["is_video"], true);
    assert_eq!(current["deletable"], true);
    assert_eq!(current["name"], "clip.MP4");
    assert_eq!(current["created_at"], photos[4].created_at.unix_timestamp());
    assert_eq!(current["trashed_on"], Value::Null);
    assert_eq!(json["photos"][0]["is_favorite"], false);

    // Defaults (30/30) clamp to the context bounds
    let json = server
        .get(
            &cookie,
            &format!(
                "/api/context/neighbors?ctx=timeline:all&photo_id={}",
                photos[0].id
            ),
        )
        .await
        .json();
    assert_eq!(
        (json["index"].as_i64(), json["start"].as_i64()),
        (Some(0), Some(0))
    );
    assert_eq!(json["photos"].as_array().unwrap().len(), 9);

    // Extending the window without a photo
    let json = server
        .get(
            &cookie,
            "/api/context/neighbors?ctx=timeline:all&start=7&limit=5",
        )
        .await
        .json();
    assert_eq!(json["index"], Value::Null);
    assert_eq!(json["start"], 7);
    assert_eq!(ids(&json["photos"]), vec![photos[7].id, photos[8].id]);

    // Not in the context
    let response = server
        .get(
            &cookie,
            &format!(
                "/api/context/neighbors?ctx=timeline:family&photo_id={}",
                photos[0].id
            ),
        )
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);

    let response = server
        .get(
            &cookie,
            "/api/context/neighbors?ctx=timeline:nope&photo_id=1",
        )
        .await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    let response = server
        .get(&cookie, "/api/context/neighbors?ctx=timeline:all")
        .await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn test_context_folder_access(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let cookie = server.login("alice").await;

    let shared = create_test_folder(server.pool(), Some("bob"), "shared").await;
    let private = create_test_folder(server.pool(), Some("bob"), "private").await;
    server
        .pool()
        .create_share(shared.id, Some("alice"), false, false, None)
        .await
        .unwrap();
    let photo = server
        .insert_photo(create_test_photo_with_time(
            0,
            Some("bob"),
            Some(shared.id),
            "a.jpg",
            datetime!(2024-06-01 10:00:00 UTC),
        ))
        .await;

    let json = server
        .get(
            &cookie,
            &format!(
                "/api/context/neighbors?ctx=folder:{}&photo_id={}",
                shared.id, photo.id
            ),
        )
        .await
        .json();
    assert_eq!(json["total"], 1);
    assert_eq!(json["photos"][0]["deletable"], false);

    let response = server
        .get(
            &cookie,
            &format!("/api/context/ids?ctx=folder:{}", private.id),
        )
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn test_context_ids(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let cookie = server.login("alice").await;

    let june = server
        .insert_photo(create_test_photo_with_time(
            0,
            Some("alice"),
            None,
            "june.jpg",
            datetime!(2024-06-01 10:00:00 UTC),
        ))
        .await;
    let may = server
        .insert_photo(create_test_photo_with_time(
            0,
            None,
            None,
            "may.jpg",
            datetime!(2024-05-01 10:00:00 UTC),
        ))
        .await;

    let json = server
        .get(&cookie, "/api/context/ids?ctx=timeline:all")
        .await
        .json();
    assert_eq!(ids(&json), vec![june.id, may.id]);

    let json = server
        .get(&cookie, "/api/context/ids?ctx=timeline:all&month=2024-05")
        .await
        .json();
    assert_eq!(ids(&json), vec![may.id]);

    let response = server
        .get(&cookie, "/api/context/ids?ctx=timeline:all&month=2024-5")
        .await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn test_photo_info(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let cookie = server.login("alice").await;

    let shared = create_test_folder(server.pool(), Some("bob"), "shared").await;
    server
        .pool()
        .create_share(shared.id, Some("alice"), false, false, None)
        .await
        .unwrap();
    let shared_photo = server
        .insert_photo(create_test_photo_with_time(
            0,
            Some("bob"),
            Some(shared.id),
            "IMG_1.jpg",
            datetime!(2024-06-01 10:00:00 UTC),
        ))
        .await;
    let public_photo = server
        .insert_photo(create_test_photo_with_time(
            0,
            None,
            None,
            "clip.mov",
            datetime!(2024-06-01 10:00:00 UTC),
        ))
        .await;
    let unshared_photo = server
        .insert_photo(create_test_photo_with_time(
            0,
            Some("carol"),
            None,
            "secret.jpg",
            datetime!(2024-06-01 10:00:00 UTC),
        ))
        .await;
    server
        .pool()
        .insert_favorite(public_photo.id, "alice")
        .await
        .unwrap();

    let json = server
        .get(&cookie, &format!("/api/photos/{}/info", shared_photo.id))
        .await
        .json();
    assert_eq!(json["photo"]["id"], shared_photo.id);
    assert_eq!(json["folder"], json!({"id": shared.id, "name": "shared"}));
    assert_eq!(
        json["owner"],
        json!({"user_id": "bob", "display_name": "Bob"})
    );
    assert_eq!(json["is_favorite"], false);
    assert_eq!(json["can_delete"], false);
    assert_eq!(json["can_move"], false);
    // The file doesn't exist: only the label is known
    assert_eq!(json["details"]["media_label"], "Image (JPG)");
    assert_eq!(json["details"]["width"], Value::Null);

    let json = server
        .get(&cookie, &format!("/api/photos/{}/info", public_photo.id))
        .await
        .json();
    assert_eq!(json["folder"], Value::Null);
    assert_eq!(json["owner"], Value::Null);
    assert_eq!(json["is_favorite"], true);
    assert_eq!(json["can_delete"], true);
    assert_eq!(json["details"]["media_label"], "Video (MOV)");

    let response = server
        .get(&cookie, &format!("/api/photos/{}/info", unshared_photo.id))
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn test_download_caching(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let cookie = server.login("alice").await;

    let photo = server
        .insert_photo(create_test_photo_with_time(
            0,
            Some("alice"),
            None,
            "Plajă.jpg",
            datetime!(2024-06-01 10:00:00 UTC),
        ))
        .await;
    server.write_file(&photo, None, b"0123456789");
    let uri = format!("/api/download/{}", photo.id);

    let response = server.get(&cookie, &uri).await;
    assert_eq!(response.status, StatusCode::OK);
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
    let etag = response.header(header::ETAG).to_string();
    assert!(etag.starts_with("W/\""));
    let last_modified = response.header(header::LAST_MODIFIED).to_string();

    let response = server.get(&cookie, &format!("{uri}?inline=1")).await;
    assert!(
        response
            .header(header::CONTENT_DISPOSITION)
            .starts_with("inline;")
    );

    let conditional = |name: header::HeaderName, value: String| {
        Request::get(&uri)
            .header(header::COOKIE, &cookie)
            .header(name, value)
            .body(Body::empty())
            .unwrap()
    };

    let response = server
        .send(conditional(header::IF_NONE_MATCH, etag.clone()))
        .await;
    assert_eq!(response.status, StatusCode::NOT_MODIFIED);
    assert!(response.body.is_empty());
    assert_eq!(response.header(header::ETAG), etag);

    let response = server
        .send(conditional(header::IF_MODIFIED_SINCE, last_modified))
        .await;
    assert_eq!(response.status, StatusCode::NOT_MODIFIED);

    let response = server
        .send(conditional(
            header::IF_NONE_MATCH,
            "W/\"other\"".to_string(),
        ))
        .await;
    assert_eq!(response.status, StatusCode::OK);

    let response = server
        .send(conditional(header::RANGE, "bytes=2-5".to_string()))
        .await;
    assert_eq!(response.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.body, b"2345");
    assert_eq!(response.header(header::CONTENT_RANGE), "bytes 2-5/10");
    assert_eq!(response.header(header::CONTENT_LENGTH), "4");

    let response = server
        .send(conditional(header::RANGE, "bytes=50-".to_string()))
        .await;
    assert_eq!(response.status, StatusCode::RANGE_NOT_SATISFIABLE);

    let response = server.request(&cookie, Method::HEAD, &uri, None).await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.header(header::CONTENT_LENGTH), "10");
    assert!(response.body.is_empty());
}

#[sqlx::test]
async fn test_preview_fallback(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let cookie = server.login("alice").await;

    let photo = server
        .insert_photo(create_test_photo_with_time(
            0,
            Some("alice"),
            None,
            "broken.jpg",
            datetime!(2024-06-01 10:00:00 UTC),
        ))
        .await;
    server.write_file(&photo, None, b"not an image");

    // The screen rendition (web only) is unavailable
    let response = server
        .get(&cookie, &format!("/api/screen/{}", photo.id))
        .await;
    assert_eq!(response.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.header(header::RETRY_AFTER), "5");
    assert_eq!(response.header(header::CACHE_CONTROL), "no-store");

    // The preview falls back to the original (owner decision, for Android), uncacheable
    let response = server
        .get(&cookie, &format!("/api/preview/{}", photo.id))
        .await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.body, b"not an image");
    assert_eq!(response.header(header::CONTENT_TYPE), "image/jpeg");
    assert_eq!(response.header(header::CACHE_CONTROL), "no-store");
    assert!(response.headers.get(header::ETAG).is_none());
    assert!(response.headers.get(header::LAST_MODIFIED).is_none());

    // Conditional requests can't pin the fallback; ranges still work
    let response = server
        .send(
            Request::get(format!("/api/preview/{}", photo.id))
                .header(header::COOKIE, &cookie)
                .header(header::IF_NONE_MATCH, "*")
                .header(header::RANGE, "bytes=0-2")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.body, b"not");

    // An existing preview is served without generating anything
    let preview_path = server
        .state
        .storage
        .resolve_preview(photo.partial_preview_path());
    std::fs::write(&preview_path, [7u8; 200]).unwrap();
    let response = server
        .get(&cookie, &format!("/api/preview/{}", photo.id))
        .await;
    assert_eq!(response.status, StatusCode::OK);
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

    // Deleting the photo permanently removes its renditions
    let screen_path = server
        .state
        .storage
        .resolve_preview(format!("screen/{}.webp", photo.id));
    std::fs::create_dir_all(screen_path.parent().unwrap()).unwrap();
    std::fs::write(&screen_path, [7u8; 200]).unwrap();
    let response = server
        .request(&cookie, Method::POST, "/api/trash", Some(json!([photo.id])))
        .await;
    assert_eq!(response.status, StatusCode::OK);
    let response = server
        .request(
            &cookie,
            Method::DELETE,
            &format!("/api/delete/{}", photo.id),
            None,
        )
        .await;
    assert_eq!(response.status, StatusCode::OK);
    assert!(!preview_path.exists());
    assert!(!screen_path.exists());
}

#[sqlx::test]
async fn test_screen_rendition(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let cookie = server.login("alice").await;

    let video = server
        .insert_photo(create_test_photo_with_time(
            0,
            Some("alice"),
            None,
            "clip.mp4",
            datetime!(2024-06-01 10:00:00 UTC),
        ))
        .await;
    let response = server
        .get(&cookie, &format!("/api/screen/{}", video.id))
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);

    // GIFs are served as they are
    let gif = server
        .insert_photo(create_test_photo_with_time(
            0,
            Some("alice"),
            None,
            "funny.gif",
            datetime!(2024-06-01 10:00:00 UTC),
        ))
        .await;
    server.write_file(&gif, None, b"GIF89a...");
    let response = server
        .get(&cookie, &format!("/api/screen/{}", gif.id))
        .await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.body, b"GIF89a...");
    assert_eq!(response.header(header::CONTENT_TYPE), "image/gif");
}

#[sqlx::test]
async fn test_folder_validation_and_conflicts(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let cookie = server.login("alice").await;

    for name in ["", "  ", "..", ".previews", "a/b", "a\\b"] {
        let response = server
            .request(
                &cookie,
                Method::POST,
                "/api/folders",
                Some(json!({"name": name, "is_public": false})),
            )
            .await;
        assert_eq!(response.status, StatusCode::BAD_REQUEST, "{name:?}");
    }
    let response = server
        .request(
            &cookie,
            Method::POST,
            "/api/folders",
            Some(json!({"name": "a/b", "is_public": false})),
        )
        .await;
    assert_eq!(response.text(), "Folder names can't contain / or \\");

    let create = |name: &'static str| {
        server.request(
            &cookie,
            Method::POST,
            "/api/folders",
            Some(json!({"name": format!(" {name} "), "is_public": false})),
        )
    };
    let first = create("Vacation").await.json();
    assert_eq!(first["name"], "Vacation");
    let second = create("Work").await.json();
    // Upsert: an existing name returns the existing folder
    assert_eq!(create("Vacation").await.json()["id"], first["id"]);

    // Neither folder has a directory yet: renaming only touches the DB
    let response = server
        .request(
            &cookie,
            Method::PATCH,
            &format!("/api/folders/{}", second["id"]),
            Some(json!({"name": "Vacation"})),
        )
        .await;
    assert_eq!(response.status, StatusCode::CONFLICT);
    assert_eq!(response.text(), "A folder with this name already exists");

    let response = server
        .request(
            &cookie,
            Method::PATCH,
            &format!("/api/folders/{}", second["id"]),
            Some(json!({"name": "a/b"})),
        )
        .await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);

    let response = server
        .request(
            &cookie,
            Method::PATCH,
            &format!("/api/folders/{}", second["id"]),
            Some(json!({"name": "Work 2024"})),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text());
    assert_eq!(response.json()["name"], "Work 2024");

    // A directory with the target name exists on disk
    let taken = server
        .state
        .storage
        .resolve_folder(Some("alice"), Some("Taken"));
    std::fs::create_dir_all(&taken).unwrap();
    let response = server
        .request(
            &cookie,
            Method::PATCH,
            &format!("/api/folders/{}", second["id"]),
            Some(json!({"name": "Taken"})),
        )
        .await;
    assert_eq!(response.status, StatusCode::CONFLICT);
}

#[sqlx::test]
async fn test_duplicate_share_and_favorite(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let cookie = server.login("alice").await;

    let folder = create_test_folder(server.pool(), Some("alice"), "mine").await;
    let share = || {
        server.request(
            &cookie,
            Method::POST,
            "/api/sharing",
            Some(json!({"folder_id": folder.id, "grantee_id": "bob"})),
        )
    };
    assert_eq!(share().await.status, StatusCode::OK);
    let response = share().await;
    assert_eq!(response.status, StatusCode::CONFLICT);
    assert_eq!(response.text(), "This person already has access");

    let photo = server
        .insert_photo(create_test_photo_with_time(
            0,
            Some("alice"),
            None,
            "a.jpg",
            datetime!(2024-06-01 10:00:00 UTC),
        ))
        .await;
    for _ in 0..2 {
        let response = server
            .request(
                &cookie,
                Method::POST,
                &format!("/api/favorite/{}", photo.id),
                None,
            )
            .await;
        assert_eq!(response.status, StatusCode::OK);
    }
    let json = server.get(&cookie, "/api/favorite").await.json();
    assert_eq!(json, json!([photo.id]));
}

#[sqlx::test]
async fn test_upload(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let cookie = server.login("alice").await;

    // Android: explicit time
    let response = server
        .upload(
            &cookie,
            "time_created=1700000000",
            "android.jpg",
            b"android",
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text());
    let json = response.json();
    assert_eq!(json["created_at"], 1_700_000_000);
    assert_eq!(json["user_id"], "alice");

    // No time: the file name is parsed
    let json = server
        .upload(&cookie, "", "IMG_20240102_030405.jpg", b"dated")
        .await
        .json();
    assert_eq!(
        json["created_at"],
        datetime!(2024-01-02 03:04:05 UTC).unix_timestamp()
    );

    // No time and nothing in the file: the fallback, then now
    let json = server
        .upload(
            &cookie,
            "fallback_time=1600000000",
            "../../etc/passwd",
            b"evil",
        )
        .await
        .json();
    assert_eq!(json["created_at"], 1_600_000_000);
    assert_eq!(json["name"], "passwd");
    let stored = server.state.storage.resolve_photo("alice/passwd");
    assert_eq!(std::fs::read(stored).unwrap(), b"evil");

    let json = server
        .upload(&cookie, "make_public=true", "now.png", b"now")
        .await
        .json();
    assert_eq!(json["user_id"], Value::Null);
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    assert!((json["created_at"].as_i64().unwrap() - now).abs() < 60);

    // Hidden names are replaced
    let json = server
        .upload(&cookie, "", ".hidden.jpg", b"hidden")
        .await
        .json();
    let name = json["name"].as_str().unwrap();
    assert!(!name.starts_with('.') && name.ends_with(".jpg"), "{name}");
}

#[sqlx::test]
async fn test_check_hashes(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;
    let bob = server.login("bob").await;

    let content = b"some photo bytes";
    let response = server.upload(&alice, "", "photo.jpg", content).await;
    assert_eq!(response.status, StatusCode::OK);

    let full = blake3::hash(content).to_hex().to_string();
    let cropped = full[..32].to_string();
    let unknown = blake3::hash(b"other").to_hex().to_string();

    let json = server
        .check_hashes(&alice, json!({"hashes": [full, cropped, unknown]}))
        .await
        .json();
    assert_eq!(json, json!({"existing": [full, cropped]}));

    // Other destinations don't have it
    let json = server
        .check_hashes(&alice, json!({"hashes": [full], "make_public": true}))
        .await
        .json();
    assert_eq!(json, json!({"existing": []}));
    let json = server
        .check_hashes(&bob, json!({"hashes": [full]}))
        .await
        .json();
    assert_eq!(json, json!({"existing": []}));

    // Into a folder: the folder owner's photos count
    let folder = create_test_folder(server.pool(), Some("alice"), "shared").await;
    server
        .pool()
        .create_share(folder.id, Some("bob"), true, false, None)
        .await
        .unwrap();
    let json = server
        .check_hashes(&alice, json!({"hashes": [full], "folder_id": folder.id}))
        .await
        .json();
    assert_eq!(json, json!({"existing": [full]}));
    // ...but for a grantee only those in the shared folder, so bob can't probe alice's photos
    let json = server
        .check_hashes(&bob, json!({"hashes": [full], "folder_id": folder.id}))
        .await
        .json();
    assert_eq!(json, json!({"existing": []}));
    let in_folder = b"shared photo bytes";
    let query = format!("folder_id={}", folder.id);
    let response = server.upload(&alice, &query, "in.jpg", in_folder).await;
    assert_eq!(response.status, StatusCode::OK);
    let in_folder = blake3::hash(in_folder).to_hex().to_string();
    let json = server
        .check_hashes(
            &bob,
            json!({"hashes": [full, in_folder], "folder_id": folder.id}),
        )
        .await
        .json();
    assert_eq!(json, json!({"existing": [in_folder]}));

    let private = create_test_folder(server.pool(), Some("carol"), "private").await;
    let response = server
        .check_hashes(&bob, json!({"hashes": [full], "folder_id": private.id}))
        .await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);

    let response = server
        .check_hashes(&alice, json!({"hashes": ["nothex"]}))
        .await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);

    let too_many = vec![full.clone(); 1001];
    let response = server
        .check_hashes(&alice, json!({"hashes": too_many}))
        .await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
}
