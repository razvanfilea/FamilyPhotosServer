use crate::model::folder::Folder;
use sqlx::{query, query_as, query_scalar, QueryBuilder, Sqlite, SqliteExecutor};

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
            returning id, owner_id, name, created_at"#,
            owner_id,
            name
        )
        .fetch_one(self)
        .await
    }

    async fn get_folder_by_id(self, id: i64) -> sqlx::Result<Option<Folder>> {
        query_as!(
            Folder,
            "select id, owner_id, name, created_at from folders where id = $1",
            id
        )
        .fetch_optional(self)
        .await
    }

    async fn get_folders_by_owner(self, owner_id: Option<&str>) -> sqlx::Result<Vec<Folder>> {
        query_as!(
            Folder,
            r#"select id, owner_id, name, created_at from folders
            where ($1 is null and owner_id is null) or owner_id = $1
            order by name"#,
            owner_id
        )
        .fetch_all(self)
        .await
    }

    async fn get_all_accessible_folders(self, user_id: &str) -> sqlx::Result<Vec<Folder>> {
        query_as!(
            Folder,
            r#"select id, owner_id, name, created_at from folders
            where owner_id = $1 or owner_id is null
            order by name"#,
            user_id
        )
        .fetch_all(self)
        .await
    }

    async fn rename_folder(self, id: i64, new_name: &str) -> sqlx::Result<()> {
        query!(
            "update folders set name = $2 where id = $1",
            id,
            new_name
        )
        .execute(self)
        .await
        .map(|_| ())
    }

    async fn delete_folder(self, id: i64) -> sqlx::Result<u64> {
        query!("delete from folders where id = $1", id)
            .execute(self)
            .await
            .map(|r| r.rows_affected())
    }

    async fn get_folder_name(self, folder_id: i64) -> sqlx::Result<Option<String>> {
        query_scalar!("select name from folders where id = $1", folder_id)
            .fetch_optional(self)
            .await
    }
}

impl<'c, E> FoldersRepo<'c> for E where E: SqliteExecutor<'c> {}

pub trait FoldersTransactionRepo {
    async fn batch_get_or_create_folders(
        &mut self,
        pairs: &[(Option<&str>, &str)],
    ) -> sqlx::Result<Vec<Folder>>;
}

impl FoldersTransactionRepo for sqlx::SqliteTransaction<'_> {
    async fn batch_get_or_create_folders(
        &mut self,
        pairs: &[(Option<&str>, &str)],
    ) -> sqlx::Result<Vec<Folder>> {
        if pairs.is_empty() {
            return Ok(Vec::new());
        }

        QueryBuilder::<Sqlite>::new("insert into folders (owner_id, name) ")
            .push_values(pairs, |mut b, (owner_id, name)| {
                b.push_bind(*owner_id).push_bind(*name);
            })
            .push(" on conflict (COALESCE(owner_id, ''), name) do update set name = excluded.name")
            .build()
            .execute(self.as_mut())
            .await?;

        let mut results = Vec::with_capacity(pairs.len());
        for (owner_id, name) in pairs {
            let coalesced = owner_id.unwrap_or("");
            let folder = query_as!(
                Folder,
                r#"select id, owner_id, name, created_at from folders
                where COALESCE(owner_id, '') = $1 and name = $2"#,
                coalesced,
                name
            )
            .fetch_one(self.as_mut())
            .await?;
            results.push(folder);
        }

        Ok(results)
    }
}

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
    async fn test_get_folders_by_owner(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        pool.get_or_create_folder(Some("user1"), "b_folder").await?;
        pool.get_or_create_folder(Some("user1"), "a_folder").await?;
        pool.get_or_create_folder(None, "public_folder").await?;

        let personal = pool.get_folders_by_owner(Some("user1")).await?;
        assert_eq!(personal.len(), 2);
        assert_eq!(personal[0].name, "a_folder");
        assert_eq!(personal[1].name, "b_folder");

        let public = pool.get_folders_by_owner(None).await?;
        assert_eq!(public.len(), 1);
        assert_eq!(public[0].name, "public_folder");

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_all_accessible_folders(pool: SqlitePool) -> sqlx::Result<()> {
        let user1 = create_test_user("user1", "User One");
        let user2 = create_test_user("user2", "User Two");
        insert_test_user(&pool, &user1).await?;
        insert_test_user(&pool, &user2).await?;

        pool.get_or_create_folder(Some("user1"), "mine").await?;
        pool.get_or_create_folder(Some("user2"), "theirs").await?;
        pool.get_or_create_folder(None, "shared").await?;

        let accessible = pool.get_all_accessible_folders("user1").await?;
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
    async fn test_delete_folder(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let folder = pool.get_or_create_folder(Some("user1"), "to_delete").await?;
        let deleted = pool.delete_folder(folder.id).await?;
        assert_eq!(deleted, 1);

        let missing = pool.get_folder_by_id(folder.id).await?;
        assert!(missing.is_none());

        let deleted_again = pool.delete_folder(folder.id).await?;
        assert_eq!(deleted_again, 0);

        Ok(())
    }

    #[sqlx::test]
    async fn test_batch_get_or_create_folders(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        pool.get_or_create_folder(Some("user1"), "existing").await?;

        let mut tx = pool.begin().await?;
        let pairs: Vec<(Option<&str>, &str)> = vec![
            (Some("user1"), "existing"),
            (Some("user1"), "new_one"),
            (None, "public"),
        ];
        let folders = tx.batch_get_or_create_folders(&pairs).await?;
        tx.commit().await?;

        assert_eq!(folders.len(), 3);
        assert_eq!(folders[0].name, "existing");
        assert_eq!(folders[1].name, "new_one");
        assert_eq!(folders[2].name, "public");
        assert_eq!(folders[2].owner_id, None);

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_folder_name(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let folder = pool.get_or_create_folder(Some("user1"), "vacation").await?;
        let name = pool.get_folder_name(folder.id).await?;
        assert_eq!(name, Some("vacation".to_string()));

        let missing = pool.get_folder_name(99999).await?;
        assert_eq!(missing, None);

        Ok(())
    }
}
