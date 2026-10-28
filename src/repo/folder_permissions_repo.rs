use crate::model::{folder::Folder, folder_permission::FolderPermission};
use sqlx::{SqliteExecutor, query_as};
use time::OffsetDateTime;

pub trait FolderPermissionsRepo<'c>: SqliteExecutor<'c> {
    #[allow(dead_code)] // TODO: used by future token-based public link access
    async fn get_permission_by_token(self, token: &str) -> sqlx::Result<Option<FolderPermission>> {
        query_as!(
            FolderPermission,
            "select * from folder_permissions where token = $1",
            token
        )
        .fetch_optional(self)
        .await
    }

    async fn get_shares_by_owner(self, owner_id: &str) -> sqlx::Result<Vec<FolderPermission>> {
        query_as!(
            FolderPermission,
            r#"select fp.* from folder_permissions fp
            inner join folders f on f.id = fp.folder_id
            where f.owner_id = $1 or f.owner_id is null
            order by fp.created_at desc"#,
            owner_id
        )
        .fetch_all(self)
        .await
    }

    async fn get_folder_by_share_id(self, share_id: i64) -> sqlx::Result<Option<Folder>> {
        query_as!(
            Folder,
            "select f.* from folders f 
            inner join folder_permissions fp on f.id = fp.folder_id 
            where fp.id = $1",
            share_id
        )
        .fetch_optional(self)
        .await
    }

    async fn get_shares_for_folder(self, folder_id: i64) -> sqlx::Result<Vec<FolderPermission>> {
        query_as!(
            FolderPermission,
            "select * from folder_permissions where folder_id = $1 order by created_at desc",
            folder_id
        )
        .fetch_all(self)
        .await
    }

    async fn create_share(
        self,
        folder_id: i64,
        grantee_id: Option<&str>,
        can_upload: bool,
        can_delete: bool,
        expires_at: Option<OffsetDateTime>,
    ) -> sqlx::Result<FolderPermission> {
        let token = if grantee_id.is_none() {
            Some(generate_token())
        } else {
            None
        };

        query_as!(
            FolderPermission,
            r#"insert into folder_permissions
                (folder_id, grantee_id, token, can_upload, can_delete, expires_at)
            values ($1, $2, $3, $4, $5, $6)
            returning id as "id!", folder_id, grantee_id, token, can_upload, can_delete,
                created_at, expires_at as "expires_at?""#,
            folder_id,
            grantee_id,
            token,
            can_upload,
            can_delete,
            expires_at
        )
        .fetch_one(self)
        .await
    }

    async fn update_share(
        self,
        share_id: i64,
        owner_id: &str,
        can_upload: bool,
        can_delete: bool,
    ) -> sqlx::Result<Option<FolderPermission>> {
        query_as!(
            FolderPermission,
            r#"update folder_permissions set can_upload = $3, can_delete = $4
            where id = $1 and folder_id in (select id from folders where owner_id = $2 or owner_id is null)
            returning id as "id!", folder_id, grantee_id, token, can_upload, can_delete,
                created_at, expires_at as "expires_at?""#,
            share_id, owner_id, can_upload, can_delete
        )
        .fetch_optional(self)
        .await
    }

    /// Sets (or clears, with `None`) the expiry of a share, under the same ownership rule as
    /// `update_share`. Returns the updated share.
    async fn set_share_expiry(
        self,
        share_id: i64,
        owner_id: &str,
        expires_at: Option<OffsetDateTime>,
    ) -> sqlx::Result<Option<FolderPermission>> {
        query_as!(
            FolderPermission,
            r#"update folder_permissions set expires_at = $3
            where id = $1 and folder_id in (select id from folders where owner_id = $2 or owner_id is null)
            returning id as "id!", folder_id, grantee_id, token, can_upload, can_delete,
                created_at, expires_at as "expires_at?""#,
            share_id,
            owner_id,
            expires_at
        )
        .fetch_optional(self)
        .await
    }

    async fn delete_share(self, share_id: i64, owner_id: &str) -> sqlx::Result<u64> {
        sqlx::query!(
            r#"delete from folder_permissions
            where id = $1 and folder_id in (select id from folders where owner_id = $2 or owner_id is null)"#,
            share_id,
            owner_id
        )
        .execute(self)
        .await
        .map(|r| r.rows_affected())
    }

    async fn delete_grantee_shares(self, folder_id: i64) -> sqlx::Result<u64> {
        sqlx::query!(
            "delete from folder_permissions where folder_id = $1 and grantee_id is not null",
            folder_id
        )
        .execute(self)
        .await
        .map(|r| r.rows_affected())
    }
}

impl<'c, E> FolderPermissionsRepo<'c> for E where E: SqliteExecutor<'c> {}

fn generate_token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::error::is_unique_violation;
    use crate::repo::tests::{create_test_folder, insert_users};
    use sqlx::SqlitePool;

    #[sqlx::test]
    async fn test_shares(pool: SqlitePool) -> sqlx::Result<()> {
        insert_users(&pool).await;
        let folder = create_test_folder(&pool, Some("alice"), "shared").await;
        let public = create_test_folder(&pool, None, "family").await;
        let bobs = create_test_folder(&pool, Some("bob"), "bobs").await;

        // With a grantee: no token
        let share = pool
            .create_share(folder.id, Some("bob"), true, false, None)
            .await?;
        assert_eq!(share.folder_id, folder.id);
        assert_eq!(share.grantee_id.as_deref(), Some("bob"));
        assert_eq!(share.token, None);
        assert!(share.can_upload && !share.can_delete);
        assert_eq!(share.expires_at, None);

        let duplicate = pool
            .create_share(folder.id, Some("bob"), false, false, None)
            .await;
        assert!(is_unique_violation(&duplicate.unwrap_err()));

        // Without a grantee: a link token
        let link = pool
            .create_share(folder.id, None, false, false, None)
            .await?;
        assert_eq!(link.grantee_id, None);
        let token = link.token.clone().unwrap();
        assert_eq!(token.len(), 32);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        let by_token = pool.get_permission_by_token(&token).await?;
        assert_eq!(by_token.map(|p| p.id), Some(link.id));
        assert!(pool.get_permission_by_token("nonexistent").await?.is_none());

        let public_share = pool
            .create_share(public.id, Some("carol"), false, false, None)
            .await?;
        let bobs_share = pool
            .create_share(bobs.id, Some("alice"), false, false, None)
            .await?;

        let ids = |shares: Vec<FolderPermission>| {
            let mut ids: Vec<i64> = shares.into_iter().map(|s| s.id).collect();
            ids.sort();
            ids
        };
        assert_eq!(
            ids(pool.get_shares_for_folder(folder.id).await?),
            [share.id, link.id]
        );
        // The shares of alice's and public folders
        assert_eq!(
            ids(pool.get_shares_by_owner("alice").await?),
            [share.id, link.id, public_share.id]
        );
        let shared_folder = pool.get_folder_by_share_id(bobs_share.id).await?;
        assert_eq!(shared_folder.map(|f| f.id), Some(bobs.id));

        // Only the folder owner (anyone for public folders) can change or revoke a share
        assert!(
            pool.update_share(share.id, "bob", true, true)
                .await?
                .is_none()
        );
        let updated = pool
            .update_share(share.id, "alice", false, true)
            .await?
            .unwrap();
        assert!(!updated.can_upload && updated.can_delete);
        assert!(
            pool.update_share(public_share.id, "bob", true, true)
                .await?
                .is_some()
        );

        assert_eq!(pool.delete_share(share.id, "bob").await?, 0);
        assert_eq!(pool.delete_share(share.id, "alice").await?, 1);
        assert_eq!(pool.delete_share(public_share.id, "bob").await?, 1);

        Ok(())
    }

    #[sqlx::test]
    async fn test_set_share_expiry(pool: SqlitePool) -> sqlx::Result<()> {
        insert_users(&pool).await;
        let folder = create_test_folder(&pool, Some("alice"), "mine").await;
        let share = pool
            .create_share(folder.id, Some("bob"), false, false, None)
            .await?;

        let expires_at = OffsetDateTime::now_utc() + time::Duration::days(3);
        let updated = pool
            .set_share_expiry(share.id, "alice", Some(expires_at))
            .await?
            .unwrap();
        assert_eq!(
            updated.expires_at.map(|t| t.unix_timestamp()),
            Some(expires_at.unix_timestamp())
        );

        // Only the owner may change it, and the flags aren't touched
        assert!(
            pool.set_share_expiry(share.id, "bob", None)
                .await?
                .is_none()
        );
        let updated = pool
            .update_share(share.id, "alice", true, false)
            .await?
            .unwrap();
        assert!(updated.expires_at.is_some());

        let cleared = pool
            .set_share_expiry(share.id, "alice", None)
            .await?
            .unwrap();
        assert!(cleared.expires_at.is_none());
        assert!(cleared.can_upload);

        Ok(())
    }
}
