use crate::model::photo::Photo;
use crate::model::photo_category::PhotoCategory;
use serde::{Deserialize, Serialize};
use sqlx::{SqliteExecutor, query_as};
use time::OffsetDateTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhotoCursor {
    pub created_at: OffsetDateTime,
    pub id: i64,
}

#[derive(Serialize)]
pub struct FolderInfo {
    pub id: i64,
    pub name: String,
    #[serde(rename = "count")]
    pub photo_count: i64,
    pub cover_photo_id: i64,
}

pub struct PaginatedPhotos {
    pub photos: Vec<Photo>,
    pub next_cursor: Option<PhotoCursor>,
    pub has_more: bool,
}

pub struct MonthSummary {
    pub max_created_at: String,
    pub count: i64,
    pub cover_photo_id: i64,
}

pub trait PhotosPageRepo<'c>: SqliteExecutor<'c> {
    async fn get_photos_paginated(
        self,
        user_id: &str,
        category: PhotoCategory,
        cursor: Option<&PhotoCursor>,
        limit: u32,
    ) -> sqlx::Result<PaginatedPhotos> {
        let fetch_limit = limit as i64 + 1;
        let cursor_created_at = cursor.map(|c| c.created_at);
        let cursor_id = cursor.map(|c| c.id);

        let photos = match category {
            PhotoCategory::Personal => {
                query_as!(
                    Photo,
                    r#"select * from photos
                    where user_id = $1
                      and trashed_on is null
                      and ($2 is null or created_at < $2 or (created_at = $2 and id < $3))
                    order by created_at desc
                    limit $4"#,
                    user_id,
                    cursor_created_at,
                    cursor_id,
                    fetch_limit
                )
                .fetch_all(self)
                .await?
            }
            PhotoCategory::Family => {
                query_as!(
                    Photo,
                    r#"select * from photos
                    where user_id is null
                      and trashed_on is null
                      and ($1 is null or created_at < $1 or (created_at = $1 and id < $2))
                    order by created_at desc
                    limit $3"#,
                    cursor_created_at,
                    cursor_id,
                    fetch_limit
                )
                .fetch_all(self)
                .await?
            }
            PhotoCategory::All => {
                query_as!(
                    Photo,
                    r#"select * from (
                        select * from photos
                        where user_id is null
                          and trashed_on is null
                          and ($1 is null or created_at < $1 or (created_at = $1 and id < $2))
                        union all
                        select * from photos
                        where user_id = $3
                          and trashed_on is null
                          and ($1 is null or created_at < $1 or (created_at = $1 and id < $2))
                    )
                    order by created_at desc
                    limit $4"#,
                    cursor_created_at,
                    cursor_id,
                    user_id,
                    fetch_limit
                )
                .fetch_all(self)
                .await?
            }
        };

        build_paginated_result(photos, limit)
    }

    async fn get_folder_photos_paginated(
        self,
        folder_id: i64,
        cursor: Option<&PhotoCursor>,
        limit: u32,
    ) -> sqlx::Result<PaginatedPhotos> {
        let fetch_limit = limit as i64 + 1;
        let cursor_created_at = cursor.map(|c| c.created_at);
        let cursor_id = cursor.map(|c| c.id);

        let photos = query_as!(
            Photo,
            r#"select * from photos
            where folder_id = $1
              and trashed_on is null
              and ($2 is null or created_at < $2 or (created_at = $2 and id < $3))
            order by created_at desc, id desc
            limit $4"#,
            folder_id,
            cursor_created_at,
            cursor_id,
            fetch_limit
        )
        .fetch_all(self)
        .await?;

        build_paginated_result(photos, limit)
    }

    async fn get_favorite_photos_paginated(
        self,
        user_id: &str,
        cursor: Option<&PhotoCursor>,
        limit: u32,
    ) -> sqlx::Result<PaginatedPhotos> {
        let fetch_limit = limit as i64 + 1;
        let cursor_created_at = cursor.map(|c| c.created_at);
        let cursor_id = cursor.map(|c| c.id);

        let photos = query_as!(
            Photo,
            r#"select p.* from photos p
            inner join favorite_photos f on p.id = f.photo_id and f.user_id = $1
            where (p.user_id is null or p.user_id = $1)
              and p.trashed_on is null
              and ($2 is null or p.created_at < $2 or (p.created_at = $2 and p.id < $3))
            order by p.created_at desc, p.id desc
            limit $4"#,
            user_id,
            cursor_created_at,
            cursor_id,
            fetch_limit
        )
        .fetch_all(self)
        .await?;

        build_paginated_result(photos, limit)
    }

    async fn get_folders_with_counts(
        self,
        user_id: &str,
        category: PhotoCategory,
    ) -> sqlx::Result<Vec<FolderInfo>> {
        match category {
            PhotoCategory::Personal => {
                query_as!(
                    FolderInfo,
                    r#"select
                        f.id as "id!: i64",
                        f.name as "name!",
                        count(*) as "photo_count!: i64",
                        max(case when rn = 1 then p.id end) as "cover_photo_id!: i64"
                    from (
                        select folder_id, id,
                               row_number() over (partition by folder_id order by created_at desc) as rn
                        from photos
                        where user_id = $1
                          and trashed_on is null
                          and folder_id is not null
                    ) p
                    inner join folders f on f.id = p.folder_id
                    group by p.folder_id
                    order by f.name"#,
                    user_id
                )
                .fetch_all(self)
                .await
            }
            PhotoCategory::Family => {
                query_as!(
                    FolderInfo,
                    r#"select
                        f.id as "id!: i64",
                        f.name as "name!",
                        count(*) as "photo_count!: i64",
                        max(case when rn = 1 then p.id end) as "cover_photo_id!: i64"
                    from (
                        select folder_id, id,
                               row_number() over (partition by folder_id order by created_at desc) as rn
                        from photos
                        where user_id is null
                          and trashed_on is null
                          and folder_id is not null
                    ) p
                    inner join folders f on f.id = p.folder_id
                    group by p.folder_id
                    order by f.name"#
                )
                .fetch_all(self)
                .await
            }
            PhotoCategory::All => {
                query_as!(
                    FolderInfo,
                    r#"select
                        f.id as "id!: i64",
                        f.name as "name!",
                        count(*) as "photo_count!: i64",
                        max(case when rn = 1 then p.id end) as "cover_photo_id!: i64"
                    from (
                        select folder_id, id,
                               row_number() over (partition by folder_id order by created_at desc) as rn
                        from photos
                        where (user_id is null or user_id = $1)
                          and trashed_on is null
                          and folder_id is not null
                    ) p
                    inner join folders f on f.id = p.folder_id
                    group by p.folder_id
                    order by f.name"#,
                    user_id
                )
                .fetch_all(self)
                .await
            }
        }
    }

    async fn get_month_summaries(
        self,
        user_id: &str,
        category: PhotoCategory,
    ) -> sqlx::Result<Vec<MonthSummary>> {
        match category {
            PhotoCategory::Personal => {
                query_as!(
                    MonthSummary,
                    r#"select
                        max(created_at) as "max_created_at!: String",
                        count(*) as "count!: i64",
                        max(case when rn = 1 then id end) as "cover_photo_id!: i64"
                    from (
                        select id, created_at,
                               row_number() over (partition by strftime('%Y-%m', created_at) order by created_at desc) as rn
                        from photos
                        where user_id = $1 and trashed_on is null
                    )
                    group by strftime('%Y-%m', created_at)
                    order by 1 desc"#,
                    user_id
                )
                .fetch_all(self)
                .await
            }
            PhotoCategory::Family => {
                query_as!(
                    MonthSummary,
                    r#"select
                        max(created_at) as "max_created_at!: String",
                        count(*) as "count!: i64",
                        max(case when rn = 1 then id end) as "cover_photo_id!: i64"
                    from (
                        select id, created_at,
                               row_number() over (partition by strftime('%Y-%m', created_at) order by created_at desc) as rn
                        from photos
                        where user_id is null and trashed_on is null
                    )
                    group by strftime('%Y-%m', created_at)
                    order by 1 desc"#
                )
                .fetch_all(self)
                .await
            }
            PhotoCategory::All => {
                query_as!(
                    MonthSummary,
                    r#"select
                        max(created_at) as "max_created_at!: String",
                        count(*) as "count!: i64",
                        max(case when rn = 1 then id end) as "cover_photo_id!: i64"
                    from (
                        select id, created_at,
                               row_number() over (partition by strftime('%Y-%m', created_at) order by created_at desc) as rn
                        from photos
                        where (user_id is null or user_id = $1) and trashed_on is null
                    )
                    group by strftime('%Y-%m', created_at)
                    order by 1 desc"#,
                    user_id
                )
                .fetch_all(self)
                .await
            }
        }
    }

    async fn get_folder_month_summaries(self, folder_id: i64) -> sqlx::Result<Vec<MonthSummary>> {
        query_as!(
            MonthSummary,
            r#"select
                max(created_at) as "max_created_at!: String",
                count(*) as "count!: i64",
                max(case when rn = 1 then id end) as "cover_photo_id!: i64"
            from (
                select id, created_at,
                       row_number() over (partition by strftime('%Y-%m', created_at) order by created_at desc) as rn
                from photos
                where folder_id = $1 and trashed_on is null
            )
            group by strftime('%Y-%m', created_at)
            order by 1 desc"#,
            folder_id
        )
        .fetch_all(self)
        .await
    }

    async fn get_trashed_photos(self, user_id: &str) -> sqlx::Result<Vec<Photo>> {
        query_as!(
            Photo,
            "select * from photos
             where (user_id is null or user_id = $1)
               and trashed_on is not null
             order by trashed_on desc",
            user_id
        )
        .fetch_all(self)
        .await
    }
}

impl<'c, E> PhotosPageRepo<'c> for E where E: SqliteExecutor<'c> {}

fn build_paginated_result(mut photos: Vec<Photo>, limit: u32) -> sqlx::Result<PaginatedPhotos> {
    let has_more = photos.len() > limit as usize;
    if has_more {
        photos.pop();
    }

    let next_cursor = if has_more {
        photos.last().map(|p| PhotoCursor {
            created_at: p.created_at,
            id: p.id,
        })
    } else {
        None
    };

    Ok(PaginatedPhotos {
        photos,
        next_cursor,
        has_more,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::PhotosTransactionRepo;
    use crate::repo::tests::{
        create_test_folder, create_test_photo, create_test_photo_with_time, create_test_user,
        insert_test_user,
    };
    use sqlx::SqlitePool;
    use time::macros::datetime;

    #[sqlx::test]
    async fn test_get_trashed_photos(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let mut tx = pool.begin().await?;

        let normal = create_test_photo(0, Some("user1"), None, "normal.jpg");
        tx.insert_photo(&normal).await?;

        let mut trashed = create_test_photo(0, Some("user1"), None, "trashed.jpg");
        trashed.trashed_on = Some(OffsetDateTime::now_utc());
        tx.insert_photo(&trashed).await?;

        let mut public_trashed = create_test_photo(0, None, None, "public_trashed.jpg");
        public_trashed.trashed_on = Some(OffsetDateTime::now_utc());
        tx.insert_photo(&public_trashed).await?;

        tx.commit().await?;

        let trashed_photos = pool.get_trashed_photos("user1").await?;
        assert_eq!(trashed_photos.len(), 2);

        let names: Vec<&str> = trashed_photos.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"trashed.jpg"));
        assert!(names.contains(&"public_trashed.jpg"));
        assert!(!names.contains(&"normal.jpg"));

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_photos_paginated(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo_with_time(
                0,
                Some("user1"),
                None,
                "p1.jpg",
                datetime!(2024-01-05 10:00:00 UTC),
            ),
            create_test_photo_with_time(
                0,
                Some("user1"),
                None,
                "p2.jpg",
                datetime!(2024-01-04 10:00:00 UTC),
            ),
            create_test_photo_with_time(
                0,
                Some("user1"),
                None,
                "p3.jpg",
                datetime!(2024-01-03 10:00:00 UTC),
            ),
            create_test_photo_with_time(
                0,
                None,
                None,
                "public.jpg",
                datetime!(2024-01-02 10:00:00 UTC),
            ),
        ];
        tx.insert_photos(&photos).await?;

        let mut trashed = create_test_photo(0, Some("user1"), None, "trashed.jpg");
        trashed.trashed_on = Some(OffsetDateTime::now_utc());
        tx.insert_photo(&trashed).await?;
        tx.commit().await?;

        let result = pool
            .get_photos_paginated("user1", PhotoCategory::All, None, 2)
            .await?;

        assert_eq!(result.photos.len(), 2);
        assert!(result.has_more);
        assert!(result.next_cursor.is_some());
        assert_eq!(result.photos[0].name, "p1.jpg");
        assert_eq!(result.photos[1].name, "p2.jpg");

        let cursor = result.next_cursor.as_ref().unwrap();
        let result = pool
            .get_photos_paginated("user1", PhotoCategory::All, Some(cursor), 10)
            .await?;

        assert_eq!(result.photos.len(), 2);
        assert!(!result.has_more);
        assert!(result.next_cursor.is_none());

        let result = pool
            .get_photos_paginated("user1", PhotoCategory::Personal, None, 10)
            .await?;
        assert_eq!(result.photos.len(), 3);
        assert!(result.photos.iter().all(|p| p.user_id.is_some()));

        let result = pool
            .get_photos_paginated("user1", PhotoCategory::Family, None, 10)
            .await?;
        assert_eq!(result.photos.len(), 1);
        assert!(result.photos[0].user_id.is_none());

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_folder_photos_paginated(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let vacation = create_test_folder(&pool, Some("user1"), "vacation").await;
        let public_vacation = create_test_folder(&pool, None, "vacation").await;
        let other = create_test_folder(&pool, Some("user1"), "other").await;

        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), Some(vacation.id), "v1.jpg"),
            create_test_photo(0, Some("user1"), Some(vacation.id), "v2.jpg"),
            create_test_photo(0, None, Some(public_vacation.id), "public_v.jpg"),
            create_test_photo(0, Some("user1"), Some(other.id), "o1.jpg"),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        let result = pool
            .get_folder_photos_paginated(vacation.id, None, 10)
            .await?;
        assert_eq!(result.photos.len(), 2);

        let result = pool
            .get_folder_photos_paginated(public_vacation.id, None, 10)
            .await?;
        assert_eq!(result.photos.len(), 1);

        let result = pool.get_folder_photos_paginated(99999, None, 10).await?;
        assert!(result.photos.is_empty());

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_favorite_photos_paginated(pool: SqlitePool) -> sqlx::Result<()> {
        use crate::repo::FavoritesRepo;
        use crate::repo::PhotosRepo;

        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), None, "p1.jpg"),
            create_test_photo(0, Some("user1"), None, "p2.jpg"),
            create_test_photo(0, Some("user1"), None, "p3.jpg"),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        let result = pool
            .get_favorite_photos_paginated("user1", None, 10)
            .await?;
        assert!(result.photos.is_empty());

        let ids = pool.get_all_photo_ids().await?;
        pool.insert_favorite(ids[0], "user1").await?;
        pool.insert_favorite(ids[2], "user1").await?;

        let result = pool
            .get_favorite_photos_paginated("user1", None, 10)
            .await?;
        assert_eq!(result.photos.len(), 2);

        Ok(())
    }

    #[sqlx::test]
    async fn test_get_folders_with_counts(pool: SqlitePool) -> sqlx::Result<()> {
        let user = create_test_user("user1", "Test User");
        insert_test_user(&pool, &user).await?;

        let folders = pool
            .get_folders_with_counts("user1", PhotoCategory::All)
            .await?;
        assert!(folders.is_empty());

        let folder_a = create_test_folder(&pool, Some("user1"), "folder_a").await;
        let folder_b = create_test_folder(&pool, Some("user1"), "folder_b").await;

        let mut tx = pool.begin().await?;
        let photos = vec![
            create_test_photo(0, Some("user1"), Some(folder_a.id), "a1.jpg"),
            create_test_photo(0, Some("user1"), Some(folder_a.id), "a2.jpg"),
            create_test_photo(0, Some("user1"), Some(folder_b.id), "b1.jpg"),
            create_test_photo(0, Some("user1"), None, "no_folder.jpg"),
        ];
        tx.insert_photos(&photos).await?;
        tx.commit().await?;

        let folders = pool
            .get_folders_with_counts("user1", PhotoCategory::All)
            .await?;
        assert_eq!(folders.len(), 2);

        let folder_a_info = folders.iter().find(|f| f.name == "folder_a").unwrap();
        assert_eq!(folder_a_info.photo_count, 2);

        Ok(())
    }
}
