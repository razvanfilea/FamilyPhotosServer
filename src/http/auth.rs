use crate::http::error::HttpError;
use crate::http::utils::AuthSession;
use crate::model::user::User;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;

/// An extractor that ensures the user is authenticated.
/// Returns the authenticated User, or responds with 401 Unauthorized.
pub struct AuthenticatedUser(pub User);

impl std::ops::Deref for AuthenticatedUser {
    type Target = User;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S> FromRequestParts<S> for AuthenticatedUser
where
    S: Send + Sync,
{
    type Rejection = HttpError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth_session = AuthSession::from_request_parts(parts, state)
            .await
            .map_err(|_| HttpError::Unauthorized)?;

        auth_session
            .user
            .map(AuthenticatedUser)
            .ok_or(HttpError::Unauthorized)
    }
}
