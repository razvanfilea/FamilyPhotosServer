use super::{Client, PASSWORD, TestServer};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::json;
use sqlx::SqlitePool;

const NEW_PASSWORD: &str = "a-new-password";

async fn login_status(server: &TestServer, user_id: &str, password: &str) -> StatusCode {
    server
        .send(
            Request::post("/api/login")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!("user_id={user_id}&password={password}")))
                .unwrap(),
        )
        .await
        .status
}

#[sqlx::test]
async fn test_change_password_validation(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;

    for (current, new, status, message) in [
        (
            "wrong",
            NEW_PASSWORD,
            StatusCode::BAD_REQUEST,
            "Current password is incorrect",
        ),
        (
            PASSWORD,
            "short",
            StatusCode::BAD_REQUEST,
            "Password must be at least 8 characters",
        ),
        (
            PASSWORD,
            // 7 characters, but more than 8 bytes
            "pässwör",
            StatusCode::BAD_REQUEST,
            "Password must be at least 8 characters",
        ),
    ] {
        let response = alice
            .post(
                "/api/profile/password",
                json!({"current_password": current, "new_password": new}),
            )
            .await;
        assert_eq!(response.status, status, "{current} {new}");
        assert_eq!(response.text(), message, "{current} {new}");
    }

    // Nothing changed
    assert_eq!(
        login_status(&server, "alice", PASSWORD).await,
        StatusCode::OK
    );

    let anonymous = server.anonymous();
    anonymous
        .post(
            "/api/profile/password",
            json!({"current_password": PASSWORD, "new_password": NEW_PASSWORD}),
        )
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn test_change_password(pool: SqlitePool) {
    let server = TestServer::new(pool).await;
    let alice = server.login("alice").await;
    let other_session = server.login("alice").await;
    let bob = server.login("bob").await;

    let response = alice
        .post(
            "/api/profile/password",
            json!({"current_password": PASSWORD, "new_password": NEW_PASSWORD}),
        )
        .await
        .assert_status(StatusCode::NO_CONTENT);

    // The session that changed it stays valid through the new cookie, others end
    let cookie = response.header(header::SET_COOKIE).split(';').next();
    let renewed = Client {
        server: &server,
        cookie: cookie.unwrap().to_string(),
    };
    renewed
        .get("/api/profile")
        .await
        .assert_status(StatusCode::OK);
    other_session
        .get("/api/profile")
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
    bob.get("/api/profile").await.assert_status(StatusCode::OK);

    assert_eq!(
        login_status(&server, "alice", PASSWORD).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login_status(&server, "alice", NEW_PASSWORD).await,
        StatusCode::OK
    );
}
