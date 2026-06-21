use crate::model::folder::Folder;
use sqlx::{SqliteExecutor, query, query_as, query_scalar};
use std::collections::HashMap;

pub trait FoldersRepo<'c>: SqliteExecutor<'c> {
    async fn upsert_folder(
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

    async fn get_folder(self, id: i64) -> sqlx::Result<Option<Folder>> {
        query_as!(Folder, "select * from folders where id = $1", id)
            .fetch_optional(self)
            .await
    }

    async fn get_accessible_folder(self, user_id: &str, id: i64) -> sqlx::Result<Option<Folder>> {
        query_as!(
            Folder,
            "select * from folders where id = $1 and (owner_id is null or owner_id = $2)",
            id,
            user_id
        )
        .fetch_optional(self)
        .await
    }

    async fn get_accessible_folders(self, user_id: &str) -> sqlx::Result<Vec<Folder>> {
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
    use crate::repo::tests::{create_test_folder, create_test_user, insert_test_user};
    use sqlx::SqlitePool;

    #[sqlx::test]
    async fn test_upsert_folder(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let folder = create_test_folder(&pool, Some("user1"), "vacation").await;
        assert_eq!(folder.name, "vacation");
        assert_eq!(folder.owner_id, Some("user1".to_string()));
        assert!(folder.id > 0);

        let same_folder = create_test_folder(&pool, Some("user1"), "vacation").await;
        assert_eq!(same_folder.id, folder.id);

        let different_folder = create_test_folder(&pool, Some("user1"), "work").await;
        assert_ne!(different_folder.id, folder.id);

        Ok(())
    }

    #[sqlx::test]
    async fn test_public_folder(pool: SqlitePool) -> sqlx::Result<()> {
        let folder = create_test_folder(&pool, None, "family").await;
        assert_eq!(folder.name, "family");
        assert_eq!(folder.owner_id, None);

        let same = create_test_folder(&pool, None, "family").await;
        assert_eq!(same.id, folder.id);

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_folder(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let folder = create_test_folder(&pool, Some("user1"), "photos").await;
        let fetched = pool.get_folder(folder.id).await?;
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().name, "photos");

        let missing = pool.get_folder(99999).await?;
        assert!(missing.is_none());

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_accessible_folders(pool: SqlitePool) -> sqlx::Result<()> {
        let user1 = create_test_user("user1", "User One");
        let user2 = create_test_user("user2", "User Two");
        insert_test_user(&pool, &user1).await?;
        insert_test_user(&pool, &user2).await?;

        create_test_folder(&pool, Some("user1"), "mine").await;
        create_test_folder(&pool, Some("user2"), "theirs").await;
        create_test_folder(&pool, None, "shared").await;

        let accessible = pool.get_accessible_folders("user1").await?;
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

        let folder = create_test_folder(&pool, Some("user1"), "old_name").await;
        pool.rename_folder(folder.id, "new_name").await?;

        let renamed = pool.get_folder(folder.id).await?.unwrap();
        assert_eq!(renamed.name, "new_name");

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_folder_name(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let folder = create_test_folder(&pool, Some("user1"), "vacation").await;
        let name = pool.get_folder_name(Some(folder.id)).await?;
        assert_eq!(name, Some("vacation".to_string()));

        let missing = pool.get_folder_name(Some(99999)).await?;
        assert_eq!(missing, None);

        let none = pool.get_folder_name(None).await?;
        assert_eq!(none, None);

        Ok(())
    }
}
