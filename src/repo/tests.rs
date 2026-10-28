//! Shared test fixtures for the repository layer (also used by the handler tests)

use crate::model::folder::Folder;
use crate::model::photo::Photo;
use crate::model::user::User;
use crate::repo::{FolderPermissionsRepo, FoldersRepo, LARGE_FILE_MIN_SIZE, PhotosTransactionRepo};
use sqlx::SqlitePool;
use std::collections::HashMap;
use time::macros::datetime;
use time::{Duration, OffsetDateTime};

/// `t - by`, clamped to midnight so the result stays on the same UTC day as `t`.
/// Same-day comparisons are where mixing RFC 3339 and SQLite datetime text breaks.
pub fn earlier_same_utc_day(t: OffsetDateTime, by: Duration) -> OffsetDateTime {
    let t = t.to_offset(time::UtcOffset::UTC);
    (t - by).max(t.replace_time(time::Time::MIDNIGHT))
}

/// The users of every fixture: `(id, display name)`
pub const USERS: [(&str, &str); 3] = [("alice", "Alice"), ("bob", "Bob"), ("carol", "Carol")];

/// A photo taken on 2024-06-01 10:00 UTC. Customise it with the struct update syntax:
/// `Photo { folder_id: Some(id), ..photo(Some("alice"), "a.jpg") }`
pub fn photo(user_id: Option<&str>, name: &str) -> Photo {
    Photo {
        id: 0,
        user_id: user_id.map(String::from),
        name: name.to_string(),
        created_at: datetime!(2024-06-01 10:00 UTC),
        file_size: 1024,
        folder_id: None,
        thumb_hash: None,
        trashed_on: None,
    }
}

/// Inserts the photo (and its events) in its own transaction
pub async fn insert_photo(pool: &SqlitePool, photo: Photo) -> Photo {
    let mut tx = pool.begin().await.unwrap();
    let photo = tx.insert_photo(&photo).await.unwrap();
    tx.commit().await.unwrap();
    photo
}

pub fn create_test_user(id: &str, display_name: &str) -> User {
    User {
        id: id.to_string(),
        name: display_name.to_string(),
        password_hash: "$argon2id$v=19$m=19456,t=2,p=1$test$testhash".to_string(),
    }
}

pub async fn insert_test_user(pool: &SqlitePool, user: &User) -> sqlx::Result<()> {
    sqlx::query!(
        "insert into users (id, name, password_hash) values ($1, $2, $3)",
        user.id,
        user.name,
        user.password_hash
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Inserts [`USERS`]
pub async fn insert_users(pool: &SqlitePool) {
    for (id, name) in USERS {
        insert_test_user(pool, &create_test_user(id, name))
            .await
            .unwrap();
    }
}

pub async fn create_test_folder(pool: &SqlitePool, owner_id: Option<&str>, name: &str) -> Folder {
    let id = pool
        .upsert_folder(owner_id, Some(name))
        .await
        .unwrap()
        .unwrap();
    pool.get_folder(id).await.unwrap().unwrap()
}

/// Photos in `Dataset` of at least this size are in the large files context
pub const BIG: i64 = LARGE_FILE_MIN_SIZE + 1;

/// Every access case, from alice's point of view. The users must exist ([`insert_users`]).
///
/// Folders: alice's `mine`; bob's `shared` (shared with alice, view only), `shared-delete`
/// (upload + delete, expires in a few hours), `expired` (upload + delete, expired a few hours ago,
/// on the same UTC day) and `private` (not shared).
///
/// Photos (by name):
/// - own: `own_1`, `own_2` (in `mine`), `own_old`, `own_trashed`, `own_big`
/// - public: `public_a` and `public_b` (equal timestamps), `public_trashed`, `public_big`
/// - bob's shared folders: `shared_1`, `shared_trashed`, `shared_delete_1` (big),
///   `shared_delete_trashed`, `expired_1` (big)
/// - no access: `unshared_1` (big, in `private`), `bob_root`, `carol_root`
pub struct Dataset {
    ids: HashMap<&'static str, i64>,
    pub own_folder: i64,
    pub shared_folder: i64,
    pub shared_delete_folder: i64,
    pub expired_folder: i64,
    pub unshared_folder: i64,
}

impl Dataset {
    pub async fn create(pool: &SqlitePool) -> Self {
        let own_folder = create_test_folder(pool, Some("alice"), "mine").await.id;
        let shared_folder = create_test_folder(pool, Some("bob"), "shared").await.id;
        let shared_delete_folder = create_test_folder(pool, Some("bob"), "shared-delete")
            .await
            .id;
        let expired_folder = create_test_folder(pool, Some("bob"), "expired").await.id;
        let unshared_folder = create_test_folder(pool, Some("bob"), "private").await.id;

        let now = OffsetDateTime::now_utc();
        pool.create_share(shared_folder, Some("alice"), false, false, None)
            .await
            .unwrap();
        pool.create_share(
            shared_delete_folder,
            Some("alice"),
            true,
            true,
            Some(now + Duration::hours(3)),
        )
        .await
        .unwrap();
        // Expired shares can't be inserted directly (check constraint), so expire it afterwards
        let expired = pool
            .create_share(
                expired_folder,
                Some("alice"),
                true,
                true,
                Some(now + Duration::days(1)),
            )
            .await
            .unwrap();
        sqlx::query("update folder_permissions set created_at = ?, expires_at = ? where id = ?")
            .bind(now - Duration::days(10))
            .bind(earlier_same_utc_day(now, Duration::hours(3)))
            .bind(expired.id)
            .execute(pool)
            .await
            .unwrap();

        let (alice, bob, carol) = (Some("alice"), Some("bob"), Some("carol"));
        let trashed = Some(datetime!(2024-08-01 10:00 UTC));
        #[rustfmt::skip]
        let rows = [
            ("own_1", alice, None, datetime!(2024-06-10 10:00 UTC), 10, None),
            ("own_2", alice, Some(own_folder), datetime!(2024-05-03 09:00 UTC), 20, None),
            ("own_old", alice, None, datetime!(2022-01-01 00:00 UTC), 30, None),
            ("own_trashed", alice, None, datetime!(2024-06-01 10:00 UTC), 40, trashed),
            ("own_big", alice, None, datetime!(2023-12-31 23:59:59 UTC), BIG + 5, None),
            ("public_a", None, None, datetime!(2024-06-05 12:00 UTC), 50, None),
            ("public_b", None, None, datetime!(2024-06-05 12:00 UTC), 60, None),
            ("public_trashed", None, None, datetime!(2024-02-01 10:00 UTC), 70,
                Some(datetime!(2024-07-01 10:00 UTC))),
            ("public_big", None, None, datetime!(2024-03-01 10:00 UTC), BIG + 10, None),
            ("shared_1", bob, Some(shared_folder), datetime!(2024-06-20 10:00 UTC), 80, None),
            ("shared_trashed", bob, Some(shared_folder), datetime!(2024-06-21 10:00 UTC), 90,
                trashed),
            ("shared_delete_1", bob, Some(shared_delete_folder), datetime!(2024-04-02 10:00 UTC),
                BIG, None),
            ("shared_delete_trashed", bob, Some(shared_delete_folder),
                datetime!(2024-04-03 10:00 UTC), 100, Some(datetime!(2024-09-01 10:00 UTC))),
            ("expired_1", bob, Some(expired_folder), datetime!(2024-06-15 10:00 UTC), BIG, None),
            ("unshared_1", bob, Some(unshared_folder), datetime!(2024-06-16 10:00 UTC), BIG, None),
            ("bob_root", bob, None, datetime!(2024-06-17 10:00 UTC), 110, None),
            ("carol_root", carol, None, datetime!(2024-06-18 10:00 UTC), 120, None),
        ];

        let mut ids = HashMap::new();
        for (name, user_id, folder_id, created_at, file_size, trashed_on) in rows {
            let photo = Photo {
                folder_id,
                created_at,
                file_size,
                trashed_on,
                ..photo(user_id, name)
            };
            ids.insert(name, insert_photo(pool, photo).await.id);
        }

        Self {
            ids,
            own_folder,
            shared_folder,
            shared_delete_folder,
            expired_folder,
            unshared_folder,
        }
    }

    pub fn id(&self, name: &str) -> i64 {
        self.ids[name]
    }

    /// The dataset names of the photos with these ids
    pub fn names(&self, ids: impl IntoIterator<Item = i64>) -> Vec<&'static str> {
        ids.into_iter()
            .map(|id| {
                *self
                    .ids
                    .iter()
                    .find(|(_, v)| **v == id)
                    .unwrap_or_else(|| panic!("id {id} is not in the Dataset"))
                    .0
            })
            .collect()
    }
}
