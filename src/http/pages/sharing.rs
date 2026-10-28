use crate::http::AppStateRef;
use crate::http::auth::AuthenticatedUser;
use crate::http::error::{HttpError, HttpResult};
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::views::{DateStyle, format_date};
use crate::model::folder_permission::FolderPermission;
use crate::model::user::User;
use crate::repo::{FolderPermissionsRepo, FolderType, FoldersRepo};
use askama::Template;
use axum::extract::{Path, State};
use axum::response::Response;
use std::collections::HashMap;
use time::OffsetDateTime;

/// A person the folder is shared with
#[derive(Debug, Clone)]
struct MemberView {
    share_id: i64,
    user_id: String,
    display_name: String,
    can_upload: bool,
    can_delete: bool,
    /// `view`, `upload` or `upload-delete`
    permission: &'static str,
    permission_label: &'static str,
    /// "Expires 12 Oct 2026" / "Expired 12 Oct 2026"
    expiry_label: Option<String>,
    expired: bool,
}

#[derive(Debug, Clone)]
struct UserView {
    id: String,
    display_name: String,
}

#[derive(Template)]
#[template(path = "sharing/dialog.html")]
struct SharingDialogTemplate {
    folder_id: i64,
    folder_name: String,
    is_public: bool,
    /// The user owns the (private) folder and may change who it is shared with
    can_modify: bool,
    owner_name: String,
    members: Vec<MemberView>,
    /// Members whose share hasn't expired: the folder badge's "Shared (N)"
    active_member_count: usize,
    /// People the folder could still be shared with
    addable: Vec<UserView>,
    /// Today (UTC) as `YYYY-MM-DD`, the earliest custom expiry
    today: String,
}

/// `permission`, label
fn permission_of(can_upload: bool, can_delete: bool) -> (&'static str, &'static str) {
    match (can_upload, can_delete) {
        (true, true) => ("upload-delete", "Can upload & delete"),
        (true, false) => ("upload", "Can upload"),
        // Deleting without uploading isn't offered, so the flag is ignored
        _ => ("view", "View only"),
    }
}

/// Whether a share has expired, exactly as the SQL access checks decide it:
/// `datetime(fp.expires_at) > datetime('now')` compares whole seconds (in UTC).
fn share_expired(expires_at: OffsetDateTime, now: OffsetDateTime) -> bool {
    expires_at.unix_timestamp() <= now.unix_timestamp()
}

fn member_view(
    share: FolderPermission,
    user_id: String,
    users: &HashMap<&str, &User>,
    now: OffsetDateTime,
) -> MemberView {
    let (permission, permission_label) = permission_of(share.can_upload, share.can_delete);
    let expired = share
        .expires_at
        .is_some_and(|expires_at| share_expired(expires_at, now));

    MemberView {
        share_id: share.id,
        display_name: users
            .get(user_id.as_str())
            .map_or_else(|| user_id.clone(), |user| user.name.clone()),
        user_id,
        can_upload: share.can_upload,
        can_delete: share.can_delete,
        permission,
        permission_label,
        expiry_label: share.expires_at.map(|expires_at| {
            let verb = if expired { "Expired" } else { "Expires" };
            format!("{verb} {}", format_date(expires_at, DateStyle::Day))
        }),
        expired,
    }
}

/// `GET /ui/dialogs/sharing/{folder_id}`: who a folder is shared with. The owner of a private
/// folder can change it, everyone else with access to the folder sees the list.
pub async fn sharing_dialog(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppStateRef>,
    Path(folder_id): Path<i64>,
) -> HttpResult<Response> {
    let card = state
        .read_pool
        .get_folder_card(&user.id, folder_id)
        .await?
        .ok_or(HttpError::NotFound)?;
    let shares = state.read_pool.get_shares_for_folder(folder_id).await?;
    let all_users = state.users_repo.get_users().await?;
    let users: HashMap<&str, &User> = all_users.iter().map(|u| (u.id.as_str(), u)).collect();

    let now = OffsetDateTime::now_utc();
    // Link shares (without a grantee) aren't managed here
    let members: Vec<MemberView> = shares
        .into_iter()
        .filter_map(|share| {
            let grantee = share.grantee_id.clone()?;
            Some(member_view(share, grantee, &users, now))
        })
        .collect();

    let addable = all_users
        .iter()
        .filter(|u| {
            card.owner_id.as_deref() != Some(u.id.as_str())
                && !members.iter().any(|m| m.user_id == u.id)
        })
        .map(|u| UserView {
            id: u.id.clone(),
            display_name: u.name.clone(),
        })
        .collect();

    let owner_name = card
        .owner_id
        .as_deref()
        .map(|owner| users.get(owner).map_or(owner, |u| u.name.as_str()))
        .unwrap_or_default()
        .to_string();

    SharingDialogTemplate {
        folder_id,
        folder_name: card.name,
        is_public: card.owner_id.is_none(),
        can_modify: card.folder_type == FolderType::Personal,
        owner_name,
        active_member_count: members.iter().filter(|m| !m.expired).count(),
        members,
        addable,
        today: now.date().to_string(),
    }
    .try_into_response()
}

#[cfg(test)]
mod tests {
    use super::share_expired;
    use sqlx::SqlitePool;
    use time::OffsetDateTime;
    use time::macros::datetime;

    /// The dialog's "expired" agrees with `datetime(fp.expires_at) > datetime('now')`
    #[sqlx::test]
    async fn test_share_expired_matches_sql(pool: SqlitePool) {
        let now = datetime!(2026-10-12 10:00:00.400 UTC);
        let cases: [(OffsetDateTime, bool); 6] = [
            (datetime!(2026-10-12 09:59:59.900 UTC), true),
            (datetime!(2026-10-12 10:00:00 UTC), true),
            // The same second: SQLite drops the fraction
            (datetime!(2026-10-12 10:00:00.900 UTC), true),
            (datetime!(2026-10-12 10:00:01 UTC), false),
            // Later the same day
            (datetime!(2026-10-12 18:00:00 UTC), false),
            // Another offset, the same instant as 09:00 UTC
            (datetime!(2026-10-12 11:00:00 +02:00), true),
        ];
        for (expires_at, expired) in cases {
            let sql_valid: bool = sqlx::query_scalar("select datetime(?) > datetime(?)")
                .bind(expires_at)
                .bind(now)
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(sql_valid, !expired, "SQL, {expires_at}");
            assert_eq!(
                share_expired(expires_at, now),
                expired,
                "Rust, {expires_at}"
            );
        }
    }
}
