use crate::model::folder::Folder;
use sqlx::{SqliteExecutor, query, query_as, query_scalar};
use std::collections::HashMap;

pub trait FoldersRepo<'c>: SqliteExecutor<'c> {
    async fn get_or_create_folder(
        self,
        owner_id: Option<&str>,
        name: &str,
    ) -> sqlx::Result<Folder> {
        query_as!(
            Folder,
            r#"insert into folders (owner_id, name)
            values ($1, $2)
            on conflict (COALESCE(owner_id, ''), name) do update set name = excluded.name
            returning *"#,
            owner_id,
            name
        )
        .fetch_one(self)
        .await
    }

    async fn get_or_create_folder_id(
        self,
        owner_id: Option<&str>,
        name: Option<&str>,
    ) -> sqlx::Result<Option<i64>> {
        let Some(name) = name.filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        query_scalar!(
            r#"insert into folders (owner_id, name)
            values ($1, $2)
            on conflict (COALESCE(owner_id, ''), name) do update set name = excluded.name
            returning id"#,
            owner_id,
            name
        )
        .fetch_one(self)
        .await
        .map(Some)
    }

    async fn get_folder_by_id(self, id: i64) -> sqlx::Result<Option<Folder>> {
        query_as!(Folder, "select * from folders where id = $1", id)
            .fetch_optional(self)
            .await
    }

    async fn get_folder_by_owner_and_name(
        self,
        owner_id: Option<&str>,
        name: &str,
    ) -> sqlx::Result<Option<Folder>> {
        let coalesced = owner_id.unwrap_or("");
        query_as!(
            Folder,
            r#"select id, owner_id, name, created_at from folders
            where COALESCE(owner_id, '') = $1 and name = $2"#,
            coalesced,
            name
        )
        .fetch_optional(self)
        .await
    }

    async fn get_folders_by_user_and_public(self, user_id: &str) -> sqlx::Result<Vec<Folder>> {
        query_as!(
            Folder,
            r#"select id, owner_id, name, created_at from folders
            where owner_id = $1
            union all
            select id, owner_id, name, created_at from folders
            where owner_id is null
            order by name"#,
            user_id
        )
        .fetch_all(self)
        .await
    }

    async fn rename_folder(self, id: i64, new_name: &str) -> sqlx::Result<()> {
        query!("update folders set name = $2 where id = $1", id, new_name)
            .execute(self)
            .await
            .map(|_| ())
    }

    async fn get_folder_name(self, folder_id: Option<i64>) -> sqlx::Result<Option<String>> {
        let Some(id) = folder_id else {
            return Ok(None);
        };
        query_scalar!("select name from folders where id = $1", id)
            .fetch_optional(self)
            .await
    }

    async fn get_folder_name_map(self) -> sqlx::Result<HashMap<i64, String>> {
        query_as!(Folder, "select id, owner_id, name, created_at from folders")
            .fetch_all(self)
            .await
            .map(|folders| folders.into_iter().map(|f| (f.id, f.name)).collect())
    }
}

impl<'c, E> FoldersRepo<'c> for E where E: SqliteExecutor<'c> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::tests::{create_test_user, insert_test_user};
    use sqlx::SqlitePool;

    #[sqlx::test]
    async fn test_get_or_create_folder(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let folder = pool.get_or_create_folder(Some("user1"), "vacation").await?;
        assert_eq!(folder.name, "vacation");
        assert_eq!(folder.owner_id, Some("user1".to_string()));
        assert!(folder.id > 0);

        let same_folder = pool.get_or_create_folder(Some("user1"), "vacation").await?;
        assert_eq!(same_folder.id, folder.id);

        let different_folder = pool.get_or_create_folder(Some("user1"), "work").await?;
        assert_ne!(different_folder.id, folder.id);

        Ok(())
    }

    #[sqlx::test]
    async fn test_public_folder(pool: SqlitePool) -> sqlx::Result<()> {
        let folder = pool.get_or_create_folder(None, "family").await?;
        assert_eq!(folder.name, "family");
        assert_eq!(folder.owner_id, None);

        let same = pool.get_or_create_folder(None, "family").await?;
        assert_eq!(same.id, folder.id);

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_folder_by_id(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let folder = pool.get_or_create_folder(Some("user1"), "photos").await?;
        let fetched = pool.get_folder_by_id(folder.id).await?;
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().name, "photos");

        let missing = pool.get_folder_by_id(99999).await?;
        assert!(missing.is_none());

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_folders_by_user_and_public(pool: SqlitePool) -> sqlx::Result<()> {
        let user1 = create_test_user("user1", "User One");
        let user2 = create_test_user("user2", "User Two");
        insert_test_user(&pool, &user1).await?;
        insert_test_user(&pool, &user2).await?;

        pool.get_or_create_folder(Some("user1"), "mine").await?;
        pool.get_or_create_folder(Some("user2"), "theirs").await?;
        pool.get_or_create_folder(None, "shared").await?;

        let accessible = pool.get_folders_by_user_and_public("user1").await?;
        assert_eq!(accessible.len(), 2);
        let names: Vec<&str> = accessible.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"mine"));
        assert!(names.contains(&"shared"));
        assert!(!names.contains(&"theirs"));

        Ok(())
    }

    #[sqlx::test]
    async fn test_rename_folder(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let folder = pool.get_or_create_folder(Some("user1"), "old_name").await?;
        pool.rename_folder(folder.id, "new_name").await?;

        let renamed = pool.get_folder_by_id(folder.id).await?.unwrap();
        assert_eq!(renamed.name, "new_name");

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_folder_name(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let folder = pool.get_or_create_folder(Some("user1"), "vacation").await?;
        let name = pool.get_folder_name(Some(folder.id)).await?;
        assert_eq!(name, Some("vacation".to_string()));

        let missing = pool.get_folder_name(Some(99999)).await?;
        assert_eq!(missing, None);

        let none = pool.get_folder_name(None).await?;
        assert_eq!(none, None);

        Ok(())
    }
}
