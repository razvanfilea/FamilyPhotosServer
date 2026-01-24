use crate::model::folder_permission::FolderPermission;
use sqlx::{SqliteExecutor, query_as};
use time::OffsetDateTime;

pub trait FolderPermissionsRepo<'c>: SqliteExecutor<'c> {
    async fn get_permission_by_token(self, token: &str) -> sqlx::Result<Option<FolderPermission>> {
        query_as!(
            FolderPermission,
            "select * from folder_permissions where token = $1",
            token
        )
        .fetch_optional(self)
        .await
    }

    async fn get_permission_by_id(self, id: i64) -> sqlx::Result<Option<FolderPermission>> {
        query_as!(
            FolderPermission,
            "select * from folder_permissions where id = $1",
            id
        )
        .fetch_optional(self)
        .await
    }

    async fn get_shares_by_owner(self, owner_id: &str) -> sqlx::Result<Vec<FolderPermission>> {
        query_as!(
            FolderPermission,
            "select * from folder_permissions where owner_id = $1 order by created_at desc",
            owner_id
        )
        .fetch_all(self)
        .await
    }

    async fn get_shares_for_grantee(self, grantee_id: &str) -> sqlx::Result<Vec<FolderPermission>> {
        query_as!(
            FolderPermission,
            "select * from folder_permissions where grantee_id = $1 order by created_at desc",
            grantee_id
        )
        .fetch_all(self)
        .await
    }

    async fn create_share(
        self,
        owner_id: &str,
        folder_name: &str,
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
                (owner_id, folder_name, grantee_id, token, can_upload, can_delete, expires_at)
            values ($1, $2, $3, $4, $5, $6, $7)
            returning *"#,
            owner_id,
            folder_name,
            grantee_id,
            token,
            can_upload,
            can_delete,
            expires_at
        )
        .fetch_one(self)
        .await
    }

    async fn delete_share(self, share_id: i64, owner_id: &str) -> sqlx::Result<u64> {
        sqlx::query!(
            "delete from folder_permissions where id = $1 and owner_id = $2",
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
