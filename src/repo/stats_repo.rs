use crate::utils::media::is_video;
use serde::Serialize;
use sqlx::{SqliteExecutor, query};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct CountSize {
    pub count: i64,
    /// Bytes
    pub size: i64,
}

impl CountSize {
    fn add(&mut self, size: i64) {
        self.count += 1;
        self.size += size;
    }
}

/// Statistics over the user's own and public, non-trashed photos (like Android).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct PhotoStatistics {
    pub family: CountSize,
    pub personal: CountSize,
    pub images: CountSize,
    pub videos: CountSize,
    pub total: CountSize,
}

pub trait StatsRepo<'c>: SqliteExecutor<'c> {
    async fn photo_statistics(self, me: &str) -> sqlx::Result<PhotoStatistics> {
        let rows = query!(
            "select user_id, name, file_size from photos
            where (user_id is null or user_id = $1) and trashed_on is null",
            me
        )
        .fetch_all(self)
        .await?;

        let mut stats = PhotoStatistics::default();
        for row in rows {
            if row.user_id.is_none() {
                stats.family.add(row.file_size);
            } else {
                stats.personal.add(row.file_size);
            }

            if is_video(&row.name) {
                stats.videos.add(row.file_size);
            } else {
                stats.images.add(row.file_size);
            }

            stats.total.add(row.file_size);
        }

        Ok(stats)
    }
}

impl<'c, E> StatsRepo<'c> for E where E: SqliteExecutor<'c> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::photo::Photo;
    use crate::repo::FolderPermissionsRepo;
    use crate::repo::tests::{create_test_folder, insert_photo, insert_users, photo};
    use sqlx::SqlitePool;
    use time::OffsetDateTime;

    #[sqlx::test]
    async fn test_photo_statistics(pool: SqlitePool) -> sqlx::Result<()> {
        insert_users(&pool).await;
        assert_eq!(
            pool.photo_statistics("alice").await?,
            PhotoStatistics::default()
        );

        let bobs_folder = create_test_folder(&pool, Some("bob"), "shared").await;
        pool.create_share(bobs_folder.id, Some("alice"), true, true, None)
            .await?;

        let photos = [
            (Some("alice"), None, "a.jpg", 100, false),
            (Some("alice"), None, "b.MP4", 1000, false),
            (Some("alice"), None, "trashed.jpg", 5, true),
            (None, None, "family.heic", 200, false),
            (None, None, "family.mov", 2000, false),
            (None, None, "family_trashed.mov", 7, true),
            // Shared with alice, but statistics only count own + public photos
            (Some("bob"), Some(bobs_folder.id), "bob.jpg", 9, false),
        ];
        for (user_id, folder_id, name, file_size, trashed) in photos {
            let photo = Photo {
                folder_id,
                file_size,
                trashed_on: trashed.then(OffsetDateTime::now_utc),
                ..photo(user_id, name)
            };
            insert_photo(&pool, photo).await;
        }

        let cs = |count, size| CountSize { count, size };
        assert_eq!(
            pool.photo_statistics("alice").await?,
            PhotoStatistics {
                personal: cs(2, 1100),
                family: cs(2, 2200),
                images: cs(2, 300),
                videos: cs(2, 3000),
                total: cs(4, 3300),
            }
        );
        let stats = pool.photo_statistics("bob").await?;
        assert_eq!((stats.personal, stats.family), (cs(1, 9), cs(2, 2200)));

        Ok(())
    }
}
