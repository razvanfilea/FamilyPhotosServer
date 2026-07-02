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
            returning *"#,
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
            returning *"#,
            share_id, owner_id, can_upload, can_delete
        )
        .fetch_optional(self)
        .await
    }

    async fn get_grantee_permission(
        self,
        grantee_id: &str,
        folder_id: i64,
    ) -> sqlx::Result<Option<FolderPermission>> {
        query_as!(
            FolderPermission,
            "select * from folder_permissions where grantee_id = $1 and folder_id = $2",
            grantee_id,
            folder_id
        )
        .fetch_optional(self)
        .await
    }

    async fn delete_share(self, share_id: i64, owner_id: &str) -> sqlx::Result<u64> {
        sqlx::query!(
            r#"delete from folder_permissions
            where id = $1 and folder_id in (select id from folders where owner_id = $2)"#,
            share_id,
            owner_id
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
    use crate::repo::tests::{create_test_folder, create_test_user, insert_test_user};
    use sqlx::SqlitePool;

    #[sqlx::test]
    async fn test_create_share(pool: SqlitePool) -> sqlx::Result<()> {
        let owner = create_test_user("owner", "Owner");
        let grantee = create_test_user("grantee", "Grantee");
        insert_test_user(&pool, &owner).await?;
        insert_test_user(&pool, &grantee).await?;

        let folder = create_test_folder(&pool, Some("owner"), "shared").await;

        // With grantee: no token generated
        let share = pool
            .create_share(folder.id, Some("grantee"), true, false, None)
            .await?;

        assert_eq!(share.folder_id, folder.id);
        assert_eq!(share.grantee_id.as_deref(), Some("grantee"));
        assert!(share.token.is_none());
        assert!(share.can_upload);
        assert!(!share.can_delete);
        assert!(share.expires_at.is_none());

        // Without grantee: token auto-generated
        let token_share = pool
            .create_share(folder.id, None, false, false, None)
            .await?;

        assert!(token_share.grantee_id.is_none());
        let token = token_share.token.expect("token should be generated");
        assert_eq!(token.len(), 32);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_shares_by_owner(pool: SqlitePool) -> sqlx::Result<()> {
        let owner = create_test_user("owner", "Owner");
        let other = create_test_user("other", "Other");
        insert_test_user(&pool, &owner).await?;
        insert_test_user(&pool, &other).await?;

        let folder_a = create_test_folder(&pool, Some("owner"), "a").await;
        let folder_b = create_test_folder(&pool, Some("other"), "b").await;

        pool.create_share(folder_a.id, Some("other"), false, false, None)
            .await?;
        pool.create_share(folder_b.id, Some("owner"), false, false, None)
            .await?;

        let shares = pool.get_shares_by_owner("owner").await?;
        assert_eq!(shares.len(), 1);
        assert_eq!(shares[0].folder_id, folder_a.id);

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_shares_for_folder(pool: SqlitePool) -> sqlx::Result<()> {
        let owner = create_test_user("owner", "Owner");
        let grantee = create_test_user("grantee", "Grantee");
        insert_test_user(&pool, &owner).await?;
        insert_test_user(&pool, &grantee).await?;

        let folder = create_test_folder(&pool, Some("owner"), "shared").await;

        pool.create_share(folder.id, Some("grantee"), true, true, None)
            .await?;
        pool.create_share(folder.id, None, false, false, None)
            .await?;

        let shares = pool.get_shares_for_folder(folder.id).await?;
        assert_eq!(shares.len(), 2);

        Ok(())
    }

    #[sqlx::test]
    async fn test_delete_share_validates_ownership(pool: SqlitePool) -> sqlx::Result<()> {
        let owner = create_test_user("owner", "Owner");
        let other = create_test_user("other", "Other");
        insert_test_user(&pool, &owner).await?;
        insert_test_user(&pool, &other).await?;

        let folder = create_test_folder(&pool, Some("owner"), "mine").await;

        let share = pool
            .create_share(folder.id, Some("other"), false, false, None)
            .await?;

        let deleted = pool.delete_share(share.id, "other").await?;
        assert_eq!(deleted, 0);

        let deleted = pool.delete_share(share.id, "owner").await?;
        assert_eq!(deleted, 1);

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_grantee_permission(pool: SqlitePool) -> sqlx::Result<()> {
        let owner = create_test_user("owner", "Owner");
        let grantee = create_test_user("grantee", "Grantee");
        insert_test_user(&pool, &owner).await?;
        insert_test_user(&pool, &grantee).await?;

        let folder = create_test_folder(&pool, Some("owner"), "shared").await;
        let other_folder = create_test_folder(&pool, Some("owner"), "private").await;

        pool.create_share(folder.id, Some("grantee"), true, false, None)
            .await?;

        let perm = pool.get_grantee_permission("grantee", folder.id).await?;
        assert!(perm.is_some());
        assert!(perm.unwrap().can_upload);

        let perm = pool
            .get_grantee_permission("grantee", other_folder.id)
            .await?;
        assert!(perm.is_none());

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_permission_by_token(pool: SqlitePool) -> sqlx::Result<()> {
        let owner = create_test_user("owner", "Owner");
        insert_test_user(&pool, &owner).await?;

        let folder = create_test_folder(&pool, Some("owner"), "public").await;

        let share = pool
            .create_share(folder.id, None, false, false, None)
            .await?;
        let token = share.token.clone().unwrap();

        let by_token = pool.get_permission_by_token(&token).await?;
        assert!(by_token.is_some());
        assert_eq!(by_token.unwrap().id, share.id);

        let missing = pool.get_permission_by_token("nonexistent").await?;
        assert!(missing.is_none());

        Ok(())
    }
}
