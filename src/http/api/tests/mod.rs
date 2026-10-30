//! Handler-level tests: the whole router (auth included) driven with `oneshot`.

mod android;
mod context;
mod files;
mod folders;
mod photos;
mod profile;
mod upload;

use crate::db::SessionStore;
use crate::http::{AppState, AppStateRef, router};
use crate::model::photo::Photo;
use crate::model::user::User;
use crate::repo::tests::{USERS, create_test_user};
use crate::utils::password_hash::generate_hash_from_password;
use crate::utils::storage_resolver::StorageResolver;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, HeaderName, Method, Request, StatusCode, header};
use serde_json::Value;
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::LazyLock;
use tempfile::TempDir;
use tower::ServiceExt;

const PASSWORD: &str = "password";

/// Hashing is slow in debug builds, so hash once
static PASSWORD_HASH: LazyLock<String> = LazyLock::new(|| generate_hash_from_password(PASSWORD));

/// The router with a temporary storage and the users of [`USERS`], who can log in
pub(crate) struct TestServer {
    router: Router,
    state: AppStateRef,
    _storage: TempDir,
}

impl TestServer {
    pub(crate) async fn new(pool: SqlitePool) -> Self {
        let storage_dir = TempDir::new().unwrap();
        let storage = StorageResolver::new(
            storage_dir.path().join("storage"),
            storage_dir.path().join("previews"),
        );

        let session_store = SessionStore::new(pool.clone(), pool.clone());
        session_store.migrate().await.unwrap();

        let state: AppStateRef = Box::leak(Box::new(AppState::new(pool.clone(), pool, storage)));
        for (id, name) in USERS {
            let user = User {
                password_hash: PASSWORD_HASH.clone(),
                ..create_test_user(id, name)
            };
            state.users_repo.insert_user(&user).await.unwrap();
        }

        Self {
            router: router(state, session_store, Vec::new()),
            state,
            _storage: storage_dir,
        }
    }

    pub(crate) fn pool(&self) -> &SqlitePool {
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

    pub(crate) async fn login(&self, user_id: &str) -> Client<'_> {
        let response = self
            .send(
                Request::post("/api/login")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(format!("user_id={user_id}&password={PASSWORD}")))
                    .unwrap(),
            )
            .await
            .assert_status(StatusCode::OK);
        let cookie = response.header(header::SET_COOKIE).split(';').next();
        Client {
            server: self,
            cookie: cookie.unwrap().to_string(),
        }
    }

    pub(crate) fn anonymous(&self) -> Client<'_> {
        Client {
            server: self,
            cookie: String::new(),
        }
    }

    /// Writes the photo's file to the storage
    pub(crate) fn write_file(
        &self,
        photo: &Photo,
        folder_name: Option<&str>,
        content: &[u8],
    ) -> PathBuf {
        let path = self
            .state
            .storage
            .resolve_photo(photo.partial_path(folder_name));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path
    }
}

/// Sends requests with a session cookie (none for [`TestServer::anonymous`])
pub(crate) struct Client<'a> {
    server: &'a TestServer,
    cookie: String,
}

impl Client<'_> {
    async fn request(
        &self,
        method: Method,
        uri: &str,
        headers: &[(HeaderName, &str)],
        json_body: Option<Value>,
    ) -> TestResponse {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::COOKIE, &self.cookie);
        for (name, value) in headers {
            builder = builder.header(name, *value);
        }
        let body = match json_body {
            Some(body) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        self.server.send(builder.body(body).unwrap()).await
    }

    pub(crate) async fn get(&self, uri: &str) -> TestResponse {
        self.request(Method::GET, uri, &[], None).await
    }

    pub(crate) async fn get_with(&self, uri: &str, headers: &[(HeaderName, &str)]) -> TestResponse {
        self.request(Method::GET, uri, headers, None).await
    }

    async fn post(&self, uri: &str, body: Value) -> TestResponse {
        self.request(Method::POST, uri, &[], Some(body)).await
    }

    async fn put(&self, uri: &str, body: Value) -> TestResponse {
        self.request(Method::PUT, uri, &[], Some(body)).await
    }

    async fn delete(&self, uri: &str) -> TestResponse {
        self.request(Method::DELETE, uri, &[], None).await
    }

    async fn patch(&self, uri: &str, body: Value) -> TestResponse {
        self.request(Method::PATCH, uri, &[], Some(body)).await
    }

    async fn upload(&self, query: &str, file_name: &str, content: &[u8]) -> TestResponse {
        const BOUNDARY: &str = "test-boundary-1234";
        let mut body = format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .into_bytes();
        body.extend_from_slice(content);
        body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());

        self.server
            .send(
                Request::post(format!("/api/upload?{query}"))
                    .header(header::COOKIE, &self.cookie)
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

pub(crate) struct TestResponse {
    pub(crate) status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl TestResponse {
    pub(crate) fn assert_status(self, status: StatusCode) -> Self {
        assert_eq!(self.status, status, "{}", self.text());
        self
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| {
            panic!("{e}: {}", self.text());
        })
    }

    pub(crate) fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub(crate) fn header(&self, name: HeaderName) -> &str {
        self.headers
            .get(&name)
            .unwrap_or_else(|| panic!("missing {name}"))
            .to_str()
            .unwrap()
    }
}

/// The ids of a JSON array of photos or ids
fn ids(value: &Value) -> Vec<i64> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_i64().or_else(|| v.as_i64()).unwrap())
        .collect()
}
