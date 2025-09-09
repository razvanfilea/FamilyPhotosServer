use crate::model::user::{User, UserCredentials};
use crate::utils::password_hash::validate_credentials;
use argon2::password_hash;
use axum_login::{AuthnBackend, UserId};
use sqlx::{SqlitePool, query, query_as};
use tokio::task;

#[derive(Clone)]
pub struct UsersRepository {
    pool: SqlitePool,
}

impl UsersRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_user(&self, user_id: &str) -> Option<User> {
        query_as!(User, "select * from users where id = $1", user_id)
            .fetch_optional(&self.pool)
            .await
            .ok()?
    }

    pub async fn get_users(&self) -> sqlx::Result<Vec<User>> {
        query_as!(User, "select * from users")
            .fetch_all(&self.pool)
            .await
    }

    pub async fn insert_user(&self, user: &User) -> sqlx::Result<()> {
        query!(
            "insert into users (id, name, password_hash) values ($1, $2, $3)",
            user.id,
            user.name,
            user.password_hash
        )
        .execute(&self.pool)
        .await
        .map(|_| ())
    }

    pub async fn delete_user(&self, user_id: &str) -> sqlx::Result<()> {
        query!("delete from users where id = $1", user_id)
            .execute(&self.pool)
            .await
            .map(|_| ())
    }
}

impl AuthnBackend for UsersRepository {
    type User = User;
    type Credentials = UserCredentials;
    type Error = password_hash::Error;

    async fn authenticate(
        &self,
        creds: Self::Credentials,
    ) -> Result<Option<Self::User>, Self::Error> {
        if let Some(user) = self.get_user(&creds.user_id).await {
            return task::spawn_blocking(|| {
                Ok(
                    if validate_credentials(creds.password, &user.password_hash)? {
                        Some(user)
                    } else {
                        None
                    },
                )
            })
            .await
            .expect("Password verification failed unexpectedly");
        }

        Ok(None)
    }

    async fn get_user(&self, user_id: &UserId<Self>) -> Result<Option<Self::User>, Self::Error> {
        Ok(self.get_user(user_id).await)
    }
}
