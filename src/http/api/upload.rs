use axum::{
    Json, Router,
    extract::{Multipart, Query, State},
    response::IntoResponse,
    routing::post,
};
use time::OffsetDateTime;
use tokio::fs;
use tracing::{error, info};

use crate::http::{AppStateRef, auth::AuthenticatedUser};
use crate::model::photo::Photo;
use crate::repo::{FoldersRepo, PhotosHashRepo, PhotosTransactionRepo};
use crate::{
    http::error::{HttpError, HttpResult},
    model::folder::AccessibleFolder,
};
use crate::{
    http::utils::write_field_to_file,
    model::photo_hash::PhotoHash,
    utils::{crop_blake_3_hash, is_safe_path_component},
};
use sqlx::SqliteConnection;
use time::serde::timestamp;

pub fn router() -> Router<AppStateRef> {
    Router::new().route("/", post(upload_photo))
}

#[derive(Debug, serde::Deserialize)]
struct UploadDataQuery {
    #[serde(with = "timestamp")]
    time_created: OffsetDateTime,
    folder_id: Option<i64>,
    #[serde(default)]
    make_public: bool,
    hash: Option<String>,
}

struct UploadTarget {
    photo_user_id: Option<String>,
    folder: Option<AccessibleFolder>,
    /// Set when uploading into a folder shared by another user: duplicates are only
    /// looked up inside this folder, so the uploader can't probe the owner's other photos.
    dedup_folder_id: Option<i64>,
}

async fn resolve_upload_target(
    conn: &mut SqliteConnection,
    user_id: &str,
    query: &UploadDataQuery,
) -> HttpResult<UploadTarget> {
    let Some(target_folder_id) = query.folder_id else {
        return Ok(UploadTarget {
            photo_user_id: (!query.make_public).then_some(user_id.to_owned()),
            folder: None,
            dedup_folder_id: None,
        });
    };

    let folder = conn
        .get_accessible_folder(user_id, target_folder_id)
        .await?
        .ok_or(HttpError::Unauthorized)?;

    if !folder.can_upload {
        return Err(HttpError::Unauthorized);
    }

    let is_shared = folder
        .owner_id
        .as_deref()
        .is_some_and(|owner_id| owner_id != user_id);

    Ok(UploadTarget {
        photo_user_id: folder.owner_id.clone(),
        dedup_folder_id: is_shared.then_some(folder.id),
        folder: Some(folder),
    })
}

async fn upload_photo(
    State(state): State<AppStateRef>,
    Query(query): Query<UploadDataQuery>,
    AuthenticatedUser(user): AuthenticatedUser,
    mut payload: Multipart,
) -> HttpResult<impl IntoResponse> {
    let field = payload
        .next_field()
        .await
        .map_err(|e| HttpError::AnyError(Box::new(e)))?
        .ok_or_else(|| HttpError::BadRequest("Multipart is empty".to_string()))?;

    let file_name = field
        .file_name()
        .or(field.name())
        .ok_or_else(|| HttpError::BadRequest("Multipart has no name".to_string()))?
        .to_owned();

    if !is_safe_path_component(&file_name) {
        return Err(HttpError::BadRequest("Invalid file name".to_string()));
    }

    {
        let mut conn = state.read_pool.acquire().await?;
        let target = resolve_upload_target(&mut conn, &user.id, &query).await?;

        if let Ok(client_hash) = blake3::Hash::from_hex(query.hash.as_deref().unwrap_or("")) {
            let hash = crop_blake_3_hash(client_hash.as_bytes());
            if let Some(photo) = check_existing_photo_with_hash(&mut conn, &target, &hash).await? {
                return Ok(Json(photo));
            }
        }
    }

    let written_file = write_field_to_file(field, &state.storage.storage_folder).await?;

    // The upload may have taken long, so the folder could have been deleted or renamed, its
    // share revoked, or the same file uploaded by someone else: check everything again.
    let mut tx = state.write_pool.begin().await?;

    let target = resolve_upload_target(&mut tx, &user.id, &query).await?;

    if let Some(photo) =
        check_existing_photo_with_hash(&mut tx, &target, &written_file.hash).await?
    {
        return Ok(Json(photo));
    }

    let folder_name = target.folder.as_ref().map(|f| f.name.as_str());

    let mut photo = Photo {
        id: 0,
        user_id: target.photo_user_id,
        name: file_name,
        created_at: query.time_created,
        file_size: written_file.size as i64,
        folder_id: target.folder.as_ref().map(|f| f.id),
        thumb_hash: None,
        trashed_on: None,
    };

    let mut photo_path = state.storage.resolve_photo(photo.partial_path(folder_name));
    if let Some(parent) = photo_path.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent).await?;
    }

    if photo_path.exists() {
        photo.name = format!(
            "{}.{}",
            uuid::Uuid::new_v4(),
            photo_path
                .extension()
                .map(|str| str.to_string_lossy().to_string())
                .unwrap_or_default()
        );
        photo_path = state.storage.resolve_photo(photo.partial_path(folder_name));
    }

    info!("Uploading file to {}", photo_path.display());

    let photo = tx.insert_photo(&photo).await?;
    tx.insert_hash(PhotoHash {
        id: photo.id,
        hash: written_file.hash.clone(),
    })
    .await?;

    written_file.persist_to(&photo_path).await?;

    tx.commit().await.inspect_err(|_| {
        // Transaction failed, delete the file
        if let Err(e) = std::fs::remove_file(photo_path) {
            error!("Failed to remove uploaded file: {e}");
        }
    })?;

    Ok(Json(photo))
}

async fn check_existing_photo_with_hash(
    conn: &mut SqliteConnection,
    target: &UploadTarget,
    hash: &[u8],
) -> Result<Option<Photo>, HttpError> {
    let existing = match target.dedup_folder_id {
        Some(folder_id) => conn.get_photo_with_hash_in_folder(hash, folder_id).await?,
        None => {
            conn.get_photo_with_hash(hash, target.photo_user_id.as_deref())
                .await?
        }
    };

    if let Some(photo) = existing {
        let folder_name = conn.get_folder_name(photo.folder_id).await?;
        info!(
            "Photo with same hash already exists with path: {}",
            photo.partial_path(folder_name.as_deref())
        );
        return Ok(Some(photo));
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use crate::db;
    use crate::http::{AppState, router};
    use crate::utils::password_hash::generate_hash_from_password;
    use crate::utils::storage_resolver::StorageResolver;
    use axum_login::tower_sessions::SessionStore as _;
    use sqlx::SqlitePool;
    use sqlx::migrate::MigrateDatabase;
    use std::net::SocketAddr;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    struct TestServer {
        addr: SocketAddr,
        cookie: String,
        write_pool: SqlitePool,
        read_pool: SqlitePool,
        _dir: tempfile::TempDir,
    }

    async fn read_response(stream: &mut TcpStream) -> String {
        let mut buf = vec![0; 8192];
        let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf))
            .await
            .expect("response timed out")
            .unwrap();
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }

    async fn send(addr: SocketAddr, request: &str) -> String {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        read_response(&mut stream).await
    }

    fn set_cookie(response: &str) -> Option<&str> {
        response
            .lines()
            .find_map(|l| l.strip_prefix("set-cookie: "))
            .and_then(|c| c.split(';').next())
    }

    /// Starts the real router on a free port, with user `u` (password `pw`) logged in.
    async fn start_server() -> TestServer {
        let dir = tempfile::tempdir().unwrap();
        let db_url = format!("sqlite:{}", dir.path().join("test.db").display());
        sqlx::Sqlite::create_database(&db_url).await.unwrap();
        let (read_pool, write_pool) = db::init_pools(&db_url).await;
        db::run_migrations(&write_pool).await;
        sqlx::query("insert into users (id, name, password_hash) values ('u', 'U', $1)")
            .bind(generate_hash_from_password("pw"))
            .execute(&write_pool)
            .await
            .unwrap();
        let session_store = db::SessionStore::new(read_pool.clone(), write_pool.clone());
        session_store.migrate().await.unwrap();

        let storage = StorageResolver::new(dir.path().join("storage"), dir.path().join("prev"));
        let state = Box::leak(Box::new(AppState::new(
            read_pool.clone(),
            write_pool.clone(),
            storage,
        )));
        let app = router(state, session_store, vec![]);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let login = send(
            addr,
            "POST /api/login HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\
             Content-Type: application/x-www-form-urlencoded\r\nContent-Length: 21\r\n\r\n\
             user_id=u&password=pw",
        )
        .await;
        let cookie = set_cookie(&login).expect(&login).to_owned();

        TestServer {
            addr,
            cookie,
            write_pool,
            read_pool,
            _dir: dir,
        }
    }

    async fn get_profile(server: &TestServer) -> String {
        let response = send(
            server.addr,
            &format!(
                "GET /api/profile HTTP/1.1\r\nHost: x\r\nConnection: close\r\nCookie: {}\r\n\r\n",
                server.cookie
            ),
        )
        .await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        response
    }

    async fn session_expiries(server: &TestServer) -> Vec<time::OffsetDateTime> {
        sqlx::query_scalar("select expiry_date from tower_sessions")
            .fetch_all(&server.read_pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn test_session_is_not_saved_again_on_the_same_day() {
        let server = start_server().await;
        let before = session_expiries(&server).await;

        let response = get_profile(&server).await;
        assert_eq!(set_cookie(&response), None);
        assert_eq!(session_expiries(&server).await, before);
    }

    #[tokio::test]
    async fn test_session_is_extended_when_last_seen_is_older() {
        let server = start_server().await;
        let store = db::SessionStore::new(server.read_pool.clone(), server.write_pool.clone());
        let id = server.cookie.split_once('=').unwrap().1.parse().unwrap();
        let mut record = store.load(&id).await.unwrap().unwrap();
        record
            .data
            .insert("last_seen".to_owned(), serde_json::json!(0));
        record.expiry_date = time::OffsetDateTime::now_utc() + time::Duration::days(1);
        store.save(&record).await.unwrap();
        let before = session_expiries(&server).await;

        let response = get_profile(&server).await;
        assert!(set_cookie(&response).is_some(), "{response}");
        let after = session_expiries(&server).await;
        assert_eq!(after.len(), 1);
        assert!(
            after[0] > before[0] + time::Duration::days(50),
            "{before:?} {after:?}"
        );
    }

    #[tokio::test]
    async fn test_anonymous_request_creates_no_session() {
        let server = start_server().await;
        sqlx::query("delete from tower_sessions")
            .execute(&server.write_pool)
            .await
            .unwrap();

        let response = send(
            server.addr,
            "GET /api/profile HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert_eq!(set_cookie(&response), None);
        assert!(session_expiries(&server).await.is_empty());
    }

    #[tokio::test]
    async fn test_authenticated_get_does_not_need_the_write_connection() {
        let server = start_server().await;
        let _held = server.write_pool.acquire().await.unwrap();
        get_profile(&server).await;
    }

    #[tokio::test]
    async fn test_write_connection_is_free_while_upload_streams() {
        let server = start_server().await;
        let multipart = format!(
            "--B\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.jpg\"\r\n\r\n{}\r\n--B--\r\n",
            "x".repeat(1000)
        );
        let (first, second) = multipart.split_at(multipart.len() / 2);
        let mut upload = TcpStream::connect(server.addr).await.unwrap();
        let headers = format!(
            "POST /api/upload?time_created=1700000000 HTTP/1.1\r\nHost: x\r\nCookie: {}\r\n\
             Content-Type: multipart/form-data; boundary=B\r\nContent-Length: {}\r\n\r\n",
            server.cookie,
            multipart.len()
        );
        upload.write_all(headers.as_bytes()).await.unwrap();
        upload.write_all(first.as_bytes()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;

        tokio::time::timeout(
            Duration::from_secs(2),
            sqlx::query("update users set name = 'V' where id = 'u'").execute(&server.write_pool),
        )
        .await
        .expect("write connection was held during the upload")
        .unwrap();

        upload.write_all(second.as_bytes()).await.unwrap();
        let response = read_response(&mut upload).await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        let count: i64 = sqlx::query_scalar("select count(*) from photos where name = 'a.jpg'")
            .fetch_one(&server.read_pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }
}
