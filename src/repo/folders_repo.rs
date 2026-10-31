use crate::model::folder::{AccessibleFolder, Folder};
use sqlx::{SqliteExecutor, query, query_as, query_scalar};
use std::collections::HashMap;
use time::OffsetDateTime;

/// How a folder relates to the user looking at it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderType {
    /// Public folder (no owner)
    Family,
    /// The user's own folder
    Personal,
    /// Someone else's folder shared with the user
    Shared,
}

impl FolderType {
    fn of(owner_id: Option<&str>, user_id: &str) -> Self {
        match owner_id {
            None => Self::Family,
            Some(owner) if owner == user_id => Self::Personal,
            Some(_) => Self::Shared,
        }
    }
}

/// A folder the user can access, with what the folder list and folder page show about it
#[derive(Debug, Clone)]
pub struct FolderCard {
    pub id: i64,
    pub name: String,
    pub owner_id: Option<String>,
    /// Display name of the owner
    pub owner_name: Option<String>,
    pub folder_type: FolderType,
    /// Photos that aren't in the trash
    pub photo_count: i64,
    /// The newest photo that isn't in the trash
    pub cover_photo_id: Option<i64>,
    pub cover_thumb_hash: Option<Vec<u8>>,
    pub latest_photo_at: Option<OffsetDateTime>,
    pub can_upload: bool,
    pub can_delete: bool,
    /// People the folder is shared with (non-expired); only counted for the user's own folders
    pub share_count: i64,
}

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

    /// Every folder the user can access (own, public, shared with them), empty ones included
    async fn get_folder_cards(self, user_id: &str) -> sqlx::Result<Vec<FolderCard>> {
        folder_cards(self, user_id, None).await
    }

    async fn get_folder_card(
        self,
        user_id: &str,
        folder_id: i64,
    ) -> sqlx::Result<Option<FolderCard>> {
        folder_cards(self, user_id, Some(folder_id))
            .await
            .map(|cards| cards.into_iter().next())
    }

    /// Photos in the folder, trashed ones included
    async fn count_folder_photos(self, folder_id: i64) -> sqlx::Result<i64> {
        query_scalar!(
            r#"select count(*) as "count!: i64" from photos where folder_id = $1"#,
            folder_id
        )
        .fetch_one(self)
        .await
    }

    /// Deletes the folder row; its permissions and folder events are removed by the cascade
    async fn delete_folder(self, id: i64) -> sqlx::Result<u64> {
        query!("delete from folders where id = $1", id)
            .execute(self)
            .await
            .map(|result| result.rows_affected())
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

/// `folder_id` restricts the result to one folder. The access rules are those of
/// `get_accessible_folders`.
async fn folder_cards<'c>(
    executor: impl SqliteExecutor<'c>,
    user_id: &str,
    folder_id: Option<i64>,
) -> sqlx::Result<Vec<FolderCard>> {
    let rows = query!(
        r#"with accessible as (
            select f.id, f.owner_id, f.name,
                (f.owner_id = $1 or f.owner_id is null or coalesce(fp.can_upload, false)) as can_upload,
                (f.owner_id = $1 or f.owner_id is null or coalesce(fp.can_delete, false)) as can_delete
            from folders f
            left join folder_permissions fp on f.id = fp.folder_id and fp.grantee_id = $1
                and (fp.expires_at is null or datetime(fp.expires_at) > datetime('now'))
            where (f.owner_id = $1 or f.owner_id is null or fp.grantee_id is not null)
                and ($2 is null or f.id = $2)
        ), covers as (
            select a.id as folder_id,
                (select p.id from photos p where p.folder_id = a.id and p.trashed_on is null
                    order by p.created_at desc, p.id desc limit 1) as cover_id,
                (select count(*) from photos p where p.folder_id = a.id and p.trashed_on is null) as cnt
            from accessible a
        )
        select a.id as "id!: i64", a.name as "name!", a.owner_id, u.name as owner_name,
            a.can_upload as "can_upload!: bool", a.can_delete as "can_delete!: bool",
            c.cnt as "photo_count!: i64",
            c.cover_id as "cover_photo_id?", p.thumb_hash as "cover_thumb_hash?",
            p.created_at as "latest_photo_at?: OffsetDateTime",
            case when a.owner_id = $1 then (
                select count(*) from folder_permissions s
                where s.folder_id = a.id and s.grantee_id is not null
                    and (s.expires_at is null or datetime(s.expires_at) > datetime('now'))
            ) else 0 end as "share_count!: i64"
        from accessible a
        join covers c on c.folder_id = a.id
        left join photos p on p.id = c.cover_id
        left join users u on u.id = a.owner_id"#,
        user_id,
        folder_id
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| FolderCard {
            id: row.id,
            folder_type: FolderType::of(row.owner_id.as_deref(), user_id),
            name: row.name,
            owner_id: row.owner_id,
            owner_name: row.owner_name,
            photo_count: row.photo_count,
            cover_photo_id: row.cover_photo_id,
            cover_thumb_hash: row.cover_thumb_hash,
            latest_photo_at: row.latest_photo_at,
            can_upload: row.can_upload,
            can_delete: row.can_delete,
            share_count: row.share_count,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::photo::Photo;
    use crate::repo::tests::{Dataset, create_test_folder, insert_photo, insert_users, photo};
    use crate::repo::{FolderPermissionsRepo, PhotosTransactionRepo};
    use sqlx::SqlitePool;
    use time::macros::datetime;
    use time::{Duration, OffsetDateTime};

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

    /// Adds a photo with a thumbhash named after it
    async fn add_photo(
        pool: &SqlitePool,
        owner: Option<&str>,
        folder_id: i64,
        name: &str,
        created_at: OffsetDateTime,
        trashed: bool,
    ) -> i64 {
        let photo = Photo {
            folder_id: Some(folder_id),
            created_at,
            trashed_on: trashed.then(OffsetDateTime::now_utc),
            ..photo(owner, name)
        };
        let id = insert_photo(pool, photo).await.id;
        let mut tx = pool.begin().await.unwrap();
        tx.update_thumb_hashes(&[(id, name.as_bytes().to_vec())])
            .await
            .unwrap();
        tx.commit().await.unwrap();
        id
    }

    #[sqlx::test]
    async fn test_folder_cards_permissions(pool: SqlitePool) -> sqlx::Result<()> {
        insert_users(&pool).await;

        let mine = create_test_folder(&pool, Some("alice"), "mine").await.id;
        create_test_folder(&pool, None, "family").await;
        let view = create_test_folder(&pool, Some("bob"), "view").await.id;
        let editor = create_test_folder(&pool, Some("bob"), "editor").await.id;
        let expired = create_test_folder(&pool, Some("bob"), "expired").await.id;
        let hidden = create_test_folder(&pool, Some("bob"), "hidden").await.id;
        let others = create_test_folder(&pool, Some("carol"), "others").await.id;

        pool.create_share(view, Some("alice"), false, false, None)
            .await?;
        pool.create_share(editor, Some("alice"), true, true, None)
            .await?;
        pool.create_share(others, Some("bob"), true, true, None)
            .await?;
        let share = pool
            .create_share(
                expired,
                Some("alice"),
                true,
                true,
                Some(OffsetDateTime::now_utc() + Duration::days(1)),
            )
            .await?;
        // Expired shares can't be created (check constraint), so expire it afterwards
        sqlx::query("update folder_permissions set created_at = ?, expires_at = ? where id = ?")
            .bind(OffsetDateTime::now_utc() - Duration::days(10))
            .bind(OffsetDateTime::now_utc() - Duration::days(2))
            .bind(share.id)
            .execute(&pool)
            .await?;

        let cards = pool.get_folder_cards("alice").await?;
        let mut names: Vec<&str> = cards.iter().map(|c| c.name.as_str()).collect();
        names.sort();
        assert_eq!(names, ["editor", "family", "mine", "view"]);
        assert!(!names.contains(&"hidden"));

        let card = |name: &str| cards.iter().find(|c| c.name == name).unwrap();
        assert_eq!(card("mine").folder_type, FolderType::Personal);
        assert_eq!(card("family").folder_type, FolderType::Family);
        assert_eq!(card("view").folder_type, FolderType::Shared);
        assert_eq!(card("view").owner_name.as_deref(), Some("Bob"));
        assert_eq!(card("family").owner_name, None);

        assert!(card("mine").can_upload && card("mine").can_delete);
        assert!(card("family").can_upload && card("family").can_delete);
        assert!(!card("view").can_upload && !card("view").can_delete);
        assert!(card("editor").can_upload && card("editor").can_delete);

        // Bob sees his four folders (the expired and hidden ones too), the family folder and
        // carol's shared one, but not alice's
        let bobs = pool.get_folder_cards("bob").await?;
        assert_eq!(bobs.len(), 6);
        assert!(bobs.iter().any(|c| c.id == others));
        assert!(!bobs.iter().any(|c| c.id == mine));

        // Same rules as the single folder lookup
        assert!(pool.get_folder_card("alice", expired).await?.is_none());
        assert!(pool.get_folder_card("alice", hidden).await?.is_none());
        assert!(pool.get_folder_card("alice", others).await?.is_none());
        assert_eq!(pool.get_folder_card("alice", view).await?.unwrap().id, view);
        assert!(pool.get_folder_card("alice", 9999).await?.is_none());

        Ok(())
    }

    #[sqlx::test]
    async fn test_folder_cards_counts_and_cover(pool: SqlitePool) -> sqlx::Result<()> {
        insert_users(&pool).await;

        let folder = create_test_folder(&pool, Some("alice"), "trip").await.id;
        let empty = create_test_folder(&pool, Some("alice"), "empty").await.id;
        let only_trashed = create_test_folder(&pool, Some("alice"), "gone").await.id;

        let old = datetime!(2023-01-01 10:00:00 UTC);
        let newest = datetime!(2024-06-08 14:03:29 UTC);
        add_photo(&pool, Some("alice"), folder, "old.jpg", old, false).await;
        // Equal timestamps: the higher id is the cover
        add_photo(&pool, Some("alice"), folder, "tie-a.jpg", newest, false).await;
        let tie_b = add_photo(&pool, Some("alice"), folder, "tie-b.jpg", newest, false).await;
        // A newer trashed photo is neither counted nor the cover
        add_photo(
            &pool,
            Some("alice"),
            folder,
            "trashed.jpg",
            datetime!(2025-01-01 00:00:00 UTC),
            true,
        )
        .await;
        add_photo(&pool, Some("alice"), only_trashed, "t.jpg", old, true).await;

        let cards = pool.get_folder_cards("alice").await?;
        let trip = cards.iter().find(|c| c.id == folder).unwrap();
        assert_eq!(trip.photo_count, 3);
        assert_eq!(trip.cover_photo_id, Some(tie_b));
        assert_eq!(
            trip.cover_thumb_hash.as_deref(),
            Some(b"tie-b.jpg".as_slice())
        );
        assert_eq!(trip.latest_photo_at, Some(newest));

        let empty_card = cards.iter().find(|c| c.id == empty).unwrap();
        assert_eq!(empty_card.photo_count, 0);
        assert_eq!(empty_card.cover_photo_id, None);
        assert_eq!(empty_card.latest_photo_at, None);

        let trashed_card = cards.iter().find(|c| c.id == only_trashed).unwrap();
        assert_eq!(trashed_card.photo_count, 0);
        assert_eq!(trashed_card.cover_photo_id, None);

        // The server's delete check counts trashed photos, too
        assert_eq!(pool.count_folder_photos(only_trashed).await?, 1);
        assert_eq!(pool.count_folder_photos(empty).await?, 0);
        assert_eq!(pool.count_folder_photos(folder).await?, 4);

        Ok(())
    }

    #[sqlx::test]
    async fn test_folder_cards_share_count(pool: SqlitePool) -> sqlx::Result<()> {
        insert_users(&pool).await;

        let folder = create_test_folder(&pool, Some("alice"), "shared").await.id;
        let family = create_test_folder(&pool, None, "family").await.id;
        pool.create_share(folder, Some("bob"), false, false, None)
            .await?;
        pool.create_share(folder, Some("carol"), true, false, None)
            .await?;
        // Link shares don't count as people
        pool.create_share(folder, None, false, false, None).await?;
        let expiring = pool
            .create_share(
                family,
                Some("bob"),
                false,
                false,
                Some(OffsetDateTime::now_utc() + Duration::days(1)),
            )
            .await?;
        assert!(expiring.grantee_id.is_some());

        let alice = pool.get_folder_cards("alice").await?;
        assert_eq!(
            alice.iter().find(|c| c.id == folder).unwrap().share_count,
            2
        );
        // Only the user's own folders carry the count
        assert_eq!(
            alice.iter().find(|c| c.id == family).unwrap().share_count,
            0
        );

        let bob = pool.get_folder_cards("bob").await?;
        assert_eq!(bob.iter().find(|c| c.id == folder).unwrap().share_count, 0);

        Ok(())
    }

    #[sqlx::test]
    async fn test_delete_folder_cascades(pool: SqlitePool) -> sqlx::Result<()> {
        insert_users(&pool).await;

        let folder = create_test_folder(&pool, Some("alice"), "temp").await.id;
        let other = create_test_folder(&pool, Some("alice"), "other").await.id;
        pool.create_share(folder, Some("bob"), false, false, None)
            .await?;
        pool.create_share(other, Some("bob"), false, false, None)
            .await?;

        assert_eq!(pool.delete_folder(folder).await?, 1);
        assert_eq!(pool.delete_folder(folder).await?, 0);
        assert!(pool.get_folder(folder).await?.is_none());
        assert!(pool.get_shares_for_folder(folder).await?.is_empty());
        assert_eq!(pool.get_shares_for_folder(other).await?.len(), 1);

        Ok(())
    }
}
