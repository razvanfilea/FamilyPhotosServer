use crate::model::folder::{AccessibleFolder, Folder};
use sqlx::{SqliteExecutor, query, query_as, query_scalar};
use std::collections::HashMap;

pub trait FoldersRepo<'c>: SqliteExecutor<'c> {
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "The old web pages were the only users; the new UI calls it again"
        )
    )]
    async fn get_folder(self, id: i64) -> sqlx::Result<Option<Folder>> {
        query_as!(Folder, "select * from folders where id = $1", id)
            .fetch_optional(self)
            .await
    }

    async fn get_accessible_folder(
        self,
        user_id: &str,
        id: i64,
    ) -> sqlx::Result<Option<AccessibleFolder>> {
        query_as!(
            AccessibleFolder,
            r#"select f.id, f.owner_id, f.name,
                (f.owner_id = $1 or f.owner_id is null or coalesce(fp.can_upload, false)) as "can_upload!: bool",
                (f.owner_id = $1 or f.owner_id is null or coalesce(fp.can_delete, false)) as "can_delete!: bool"
            from folders f
            left join folder_permissions fp on f.id = fp.folder_id and fp.grantee_id = $1
                and (fp.expires_at is null or datetime(fp.expires_at) > datetime('now'))
            where f.id = $2 and (f.owner_id = $1 or f.owner_id is null or fp.grantee_id is not null)"#,
            user_id,
            id
        )
        .fetch_optional(self)
        .await
    }

    async fn get_accessible_folders(self, user_id: &str) -> sqlx::Result<Vec<AccessibleFolder>> {
        query_as!(
            AccessibleFolder,
            r#"select distinct f.id, f.owner_id, f.name,
                (f.owner_id = $1 or f.owner_id is null or coalesce(fp.can_upload, false)) as "can_upload!: bool",
                (f.owner_id = $1 or f.owner_id is null or coalesce(fp.can_delete, false)) as "can_delete!: bool"
            from folders f
            left join folder_permissions fp on f.id = fp.folder_id and fp.grantee_id = $1
                and (fp.expires_at is null or datetime(fp.expires_at) > datetime('now'))
            where f.owner_id = $1 or f.owner_id is null or fp.grantee_id is not null
            order by f.name"#,
            user_id
        )
        .fetch_all(self)
        .await
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

    async fn rename_folder(self, id: i64, new_name: &str) -> sqlx::Result<()> {
        query!("update folders set name = $2 where id = $1", id, new_name)
            .execute(self)
            .await
            .map(|_| ())
    }

    async fn update_folder_owner(self, id: i64, new_owner_id: Option<&str>) -> sqlx::Result<()> {
        query!(
            "update folders set owner_id = $2 where id = $1",
            id,
            new_owner_id
        )
        .execute(self)
        .await
        .map(|_| ())
    }
}

impl<'c, E> FoldersRepo<'c> for E where E: SqliteExecutor<'c> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::tests::{Dataset, create_test_folder, insert_users};
    use sqlx::SqlitePool;

    #[sqlx::test]
    async fn test_folder_crud(pool: SqlitePool) -> sqlx::Result<()> {
        insert_users(&pool).await;

        let folder = create_test_folder(&pool, Some("alice"), "vacation").await;
        assert_eq!(folder.name, "vacation");
        assert_eq!(folder.owner_id.as_deref(), Some("alice"));
        // Upsert: the same owner and name is the same folder
        let upsert = |owner_id, name| pool.upsert_folder(owner_id, name);
        assert_eq!(
            upsert(Some("alice"), Some("vacation")).await?,
            Some(folder.id)
        );
        let others = [
            upsert(Some("alice"), Some("work")).await?,
            upsert(Some("bob"), Some("vacation")).await?,
            upsert(None, Some("vacation")).await?,
        ];
        assert!(others.iter().all(|id| id.is_some_and(|id| id != folder.id)));
        assert_ne!(others[0], others[1]);
        assert_ne!(others[1], others[2]);
        let public = others[2].unwrap();
        assert_eq!(upsert(None, Some("vacation")).await?, Some(public));
        assert_eq!(upsert(Some("alice"), Some("")).await?, None);
        assert_eq!(upsert(Some("alice"), None).await?, None);

        assert!(pool.get_folder(99999).await?.is_none());
        assert_eq!(
            pool.get_folder_name(Some(folder.id)).await?.as_deref(),
            Some("vacation")
        );
        assert_eq!(pool.get_folder_name(Some(99999)).await?, None);
        assert_eq!(pool.get_folder_name(None).await?, None);

        pool.rename_folder(folder.id, "renamed").await?;
        pool.update_folder_owner(folder.id, None).await?;
        let updated = pool.get_folder(folder.id).await?.unwrap();
        assert_eq!((updated.name.as_str(), updated.owner_id), ("renamed", None));

        let names = pool.get_folder_name_map().await?;
        assert_eq!(names.len(), 4);
        assert_eq!(names[&folder.id], "renamed");

        Ok(())
    }

    #[sqlx::test]
    async fn test_accessible_folders(pool: SqlitePool) -> sqlx::Result<()> {
        insert_users(&pool).await;
        let data = Dataset::create(&pool).await;
        let public = create_test_folder(&pool, None, "family").await.id;

        // (folder, alice's (can_upload, can_delete))
        let cases = [
            (public, Some((true, true))),
            (data.own_folder, Some((true, true))),
            (data.shared_folder, Some((false, false))),
            (data.shared_delete_folder, Some((true, true))),
            (data.expired_folder, None),
            (data.unshared_folder, None),
        ];
        for (id, expected) in cases {
            let folder = pool.get_accessible_folder("alice", id).await?;
            assert_eq!(
                folder.map(|f| (f.can_upload, f.can_delete)),
                expected,
                "{id}"
            );
        }

        // By name
        let folders = pool.get_accessible_folders("alice").await?;
        let folders: Vec<_> = folders
            .iter()
            .map(|f| (f.id, f.can_upload, f.can_delete))
            .collect();
        assert_eq!(
            folders,
            [
                (public, true, true),
                (data.own_folder, true, true),
                (data.shared_folder, false, false),
                (data.shared_delete_folder, true, true),
            ]
        );

        // Carol only sees the public folder
        let folders = pool.get_accessible_folders("carol").await?;
        assert_eq!(folders.iter().map(|f| f.id).collect::<Vec<_>>(), [public]);

        Ok(())
    }
}
