use crate::http::AppStateRef;
use crate::http::api::sharing::{create_share_for, revoke_share_for, update_share_for};
use crate::http::auth::AuthenticatedUser;
use crate::http::error::{HttpError, HttpResult};
use crate::http::fragments::{ToastKind, toast_trigger};
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::views::{DateStyle, format_date};
use crate::model::folder_permission::FolderPermission;
use crate::model::folder_permission::{CreateShareRequest, UpdateShareRequest};
use crate::model::user::User;
use crate::repo::{FolderPermissionsRepo, FolderType, FoldersRepo};
use askama::Template;
use axum::Form;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::response::Response;
use serde::Deserialize;
use std::collections::HashMap;
use time::OffsetDateTime;
use time::macros::format_description;
use time::util::days_in_month;
use time::{Date, Month};

/// A person the folder is shared with
#[derive(Debug, Clone)]
struct MemberView {
    share_id: i64,
    user_id: String,
    display_name: String,
    /// `view`, `upload` or `upload-delete`
    permission: &'static str,
    permission_label: &'static str,
    /// "Expires 12 Oct 2026" / "Expired 12 Oct 2026"
    expiry_label: Option<String>,
    expired: bool,
    /// Its menu is open (the member just changed)
    open: bool,
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
        permission,
        permission_label,
        expiry_label: share.expires_at.map(|expires_at| {
            let verb = if expired { "Expired" } else { "Expires" };
            format!("{verb} {}", format_date(expires_at, DateStyle::Day))
        }),
        expired,
        open: false,
    }
}

/// `GET /ui/dialogs/sharing/{folder_id}`: who a folder is shared with. The owner of a private
/// folder can change it, everyone else with access to the folder sees the list.
pub async fn sharing_dialog(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppStateRef>,
    Path(folder_id): Path<i64>,
) -> HttpResult<Response> {
    render_dialog(state, &user.id, folder_id, None).await
}

/// The dialog, with the menu of `open_share` open
async fn render_dialog(
    state: AppStateRef,
    user_id: &str,
    folder_id: i64,
    open_share: Option<i64>,
) -> HttpResult<Response> {
    let card = state
        .read_pool
        .get_folder_card(user_id, folder_id)
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
            let mut member = member_view(share, grantee, &users, now);
            member.open = open_share == Some(member.share_id);
            Some(member)
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

/// The dialog after a change, with a toast
async fn changed(
    state: AppStateRef,
    user: &User,
    folder_id: i64,
    open_share: Option<i64>,
    message: &str,
) -> HttpResult<Response> {
    let dialog = render_dialog(state, &user.id, folder_id, open_share).await?;
    Ok((toast_trigger(message, ToastKind::Success), dialog).into_response())
}

/// The expiry an expiry field asks for. `None`: leave it as it is.
/// `Some(None)`: never expires.
fn parse_expiry(
    choice: &str,
    date: &str,
    now: OffsetDateTime,
) -> HttpResult<Option<Option<OffsetDateTime>>> {
    Ok(match choice {
        "keep" => None,
        "" | "none" => Some(None),
        "day" => Some(Some(now + time::Duration::days(1))),
        "week" => Some(Some(now + time::Duration::days(7))),
        "month" => Some(Some(one_month_later(now))),
        "custom" => {
            let date = Date::parse(date, format_description!("[year]-[month]-[day]"))
                .map_err(|_| HttpError::BadRequest("Pick an expiry date".to_string()))?;
            // The end of that day (UTC, like all dates in the app)
            Some(Some(
                date.with_hms(23, 59, 59)
                    .map_err(|e| HttpError::Internal(e.to_string()))?
                    .assume_utc(),
            ))
        }
        _ => return Err(HttpError::BadRequest("Invalid expiry".to_string())),
    })
}

/// Same day next month, or the last day of a shorter month
fn one_month_later(now: OffsetDateTime) -> OffsetDateTime {
    let (year, month) = match now.month() {
        Month::December => (now.year() + 1, Month::January),
        month => (now.year(), month.next()),
    };
    let day = now.day().min(days_in_month(month, year));
    Date::from_calendar_date(year, month, day).map_or(now, |date| now.replace_date(date))
}

#[derive(Debug, Deserialize)]
pub struct AddMemberForm {
    user_id: String,
    #[serde(default)]
    expiry: String,
    #[serde(default)]
    expiry_date: String,
}

/// `POST /ui/sharing/folder/{folder_id}/members`: shares the folder with a person (view only)
pub async fn add_member(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppStateRef>,
    Path(folder_id): Path<i64>,
    Form(form): Form<AddMemberForm>,
) -> HttpResult<Response> {
    let expires_at =
        parse_expiry(&form.expiry, &form.expiry_date, OffsetDateTime::now_utc())?.flatten();
    create_share_for(
        state,
        &user,
        CreateShareRequest {
            folder_id,
            grantee_id: Some(form.user_id),
            can_upload: false,
            can_delete: false,
            expires_at,
        },
    )
    .await?;
    changed(state, &user, folder_id, None, "Member added").await
}

#[derive(Debug, Deserialize)]
pub struct PermissionForm {
    /// `view`, `upload` or `upload-delete`
    permission: String,
}

/// The folder of a share the user owns (anything else is not found)
async fn owned_share_folder(state: AppStateRef, user: &User, share_id: i64) -> HttpResult<i64> {
    let folder = state
        .read_pool
        .get_folder_by_share_id(share_id)
        .await?
        .filter(|folder| folder.owner_id.as_deref() == Some(user.id.as_str()))
        .ok_or(HttpError::NotFound)?;
    Ok(folder.id)
}

/// `POST /ui/sharing/share/{share_id}/permission`: what a member may do
pub async fn set_permission(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppStateRef>,
    Path(share_id): Path<i64>,
    Form(form): Form<PermissionForm>,
) -> HttpResult<Response> {
    let (can_upload, can_delete) = match form.permission.as_str() {
        "view" => (false, false),
        "upload" => (true, false),
        "upload-delete" => (true, true),
        _ => return Err(HttpError::BadRequest("Invalid permission".to_string())),
    };
    let folder_id = owned_share_folder(state, &user, share_id).await?;
    update_share_for(
        state,
        &user,
        share_id,
        UpdateShareRequest {
            can_upload,
            can_delete,
            expires_at: None,
        },
    )
    .await?;
    changed(
        state,
        &user,
        folder_id,
        Some(share_id),
        "Permissions updated",
    )
    .await
}

#[derive(Debug, Deserialize)]
pub struct ExpiryForm {
    expiry: String,
    #[serde(default)]
    expiry_date: String,
}

/// `POST /ui/sharing/share/{share_id}/expiry`: when a member's access ends
pub async fn set_expiry(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppStateRef>,
    Path(share_id): Path<i64>,
    Form(form): Form<ExpiryForm>,
) -> HttpResult<Response> {
    let folder_id = owned_share_folder(state, &user, share_id).await?;
    let Some(expires_at) =
        parse_expiry(&form.expiry, &form.expiry_date, OffsetDateTime::now_utc())?
    else {
        // "Keep": nothing to change
        return render_dialog(state, &user.id, folder_id, Some(share_id)).await;
    };
    let share = state
        .read_pool
        .get_shares_for_folder(folder_id)
        .await?
        .into_iter()
        .find(|share| share.id == share_id)
        .ok_or(HttpError::NotFound)?;
    update_share_for(
        state,
        &user,
        share_id,
        UpdateShareRequest {
            can_upload: share.can_upload,
            can_delete: share.can_delete,
            expires_at: Some(expires_at.map(OffsetDateTime::unix_timestamp)),
        },
    )
    .await?;
    changed(
        state,
        &user,
        folder_id,
        Some(share_id),
        "Permissions updated",
    )
    .await
}

/// `DELETE /ui/sharing/share/{share_id}`: ends a member's access
pub async fn remove_member(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppStateRef>,
    Path(share_id): Path<i64>,
) -> HttpResult<Response> {
    let folder_id = owned_share_folder(state, &user, share_id).await?;
    revoke_share_for(state, &user, share_id).await?;
    changed(state, &user, folder_id, None, "Member removed").await
}

#[cfg(test)]
mod tests {
    use super::{parse_expiry, share_expired};
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

    #[test]
    fn test_parse_expiry() {
        let now = datetime!(2026-01-31 12:00:00 UTC);
        let parse = |choice, date| parse_expiry(choice, date, now).unwrap();

        assert_eq!(parse("keep", ""), None);
        assert_eq!(parse("none", ""), Some(None));
        assert_eq!(
            parse("day", ""),
            Some(Some(datetime!(2026-02-01 12:00:00 UTC)))
        );
        assert_eq!(
            parse("week", ""),
            Some(Some(datetime!(2026-02-07 12:00:00 UTC)))
        );
        // A shorter month ends on its last day
        assert_eq!(
            parse("month", ""),
            Some(Some(datetime!(2026-02-28 12:00:00 UTC)))
        );
        assert_eq!(
            parse_expiry("month", "", datetime!(2026-12-15 08:00:00 UTC)).unwrap(),
            Some(Some(datetime!(2027-01-15 08:00:00 UTC)))
        );
        // A date expires at the end of that day
        assert_eq!(
            parse("custom", "2026-10-12"),
            Some(Some(datetime!(2026-10-12 23:59:59 UTC)))
        );
        for bad in ["", "12/10/2026", "2026-13-45"] {
            assert!(parse_expiry("custom", bad, now).is_err(), "{bad:?}");
        }
        assert!(parse_expiry("year", "", now).is_err());
    }
}

#[cfg(test)]
mod ui_tests {
    use crate::http::pages::folder_tests::{App, triggers};
    use crate::repo::FolderPermissionsRepo;
    use axum::http::{Method, StatusCode};
    use sqlx::SqlitePool;

    /// The dialog's members as `(user, can_upload, can_delete, expires)`
    async fn members(app: &App, folder: i64) -> Vec<(String, bool, bool, bool)> {
        let mut members: Vec<_> = app
            .pool()
            .get_shares_for_folder(folder)
            .await
            .unwrap()
            .into_iter()
            .filter_map(|share| {
                Some((
                    share.grantee_id?,
                    share.can_upload,
                    share.can_delete,
                    share.expires_at.is_some(),
                ))
            })
            .collect();
        members.sort();
        members
    }

    fn share_id_of(app: &App, folder: i64, user: &str) -> impl Future<Output = i64> {
        let pool = app.pool().clone();
        let user = user.to_string();
        async move {
            sqlx::query_scalar(
                "select id from folder_permissions where folder_id = ? and grantee_id = ?",
            )
            .bind(folder)
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    }

    #[sqlx::test]
    async fn test_ui_sharing_changes(pool: SqlitePool) {
        let app = App::new(pool).await;
        let alice = app.login("alice").await;
        let bob = app.login("bob").await;
        let carol = app.login("carol").await;

        let folder = app.folder(Some("alice"), "Holiday").await;
        let add = format!("/ui/sharing/folder/{folder}/members");

        // Adding returns the dialog with the new member and a toast
        let response = app
            .form(&alice, Method::POST, &add, "user_id=bob&expiry=none")
            .await;
        assert_eq!(response.status, StatusCode::OK, "{}", response.body);
        assert_eq!(triggers(&response)["fp:toast"]["message"], "Member added");
        assert!(response.body.contains(r#"id="sharing-dialog""#));
        assert!(response.body.contains(r#"data-testid="sharing-member""#));
        assert_eq!(
            members(&app, folder).await,
            [("bob".into(), false, false, false)]
        );

        // Presets and custom dates become an expiry, a custom date without a date is refused
        let response = app
            .form(&alice, Method::POST, &add, "user_id=carol&expiry=week")
            .await;
        assert_eq!(response.status, StatusCode::OK);
        assert!(
            members(&app, folder)
                .await
                .contains(&("carol".into(), false, false, true))
        );
        let response = app
            .form(
                &alice,
                Method::POST,
                &add,
                "user_id=carol&expiry=custom&expiry_date=",
            )
            .await;
        assert_eq!(response.status, StatusCode::BAD_REQUEST);
        assert_eq!(response.body, "Pick an expiry date");
        // Already a member
        let response = app
            .form(&alice, Method::POST, &add, "user_id=carol&expiry=none")
            .await;
        assert_eq!(response.status, StatusCode::CONFLICT);

        // Permissions keep the menu of that member open
        let bob_share = share_id_of(&app, folder, "bob").await;
        let permission = format!("/ui/sharing/share/{bob_share}/permission");
        let response = app
            .form(
                &alice,
                Method::POST,
                &permission,
                "permission=upload-delete",
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "{}", response.body);
        assert_eq!(
            triggers(&response)["fp:toast"]["message"],
            "Permissions updated"
        );
        assert!(response.body.contains(r#"aria-expanded="true""#));
        assert!(
            members(&app, folder)
                .await
                .contains(&("bob".into(), true, true, false))
        );
        let response = app
            .form(&alice, Method::POST, &permission, "permission=admin")
            .await;
        assert_eq!(response.status, StatusCode::BAD_REQUEST);

        // The expiry
        let expiry = format!("/ui/sharing/share/{bob_share}/expiry");
        let response = app
            .form(
                &alice,
                Method::POST,
                &expiry,
                "expiry=custom&expiry_date=2099-01-15",
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "{}", response.body);
        assert!(response.body.contains("Expires 15 Jan 2099"));
        // Flags stay as they were
        assert!(
            members(&app, folder)
                .await
                .contains(&("bob".into(), true, true, true))
        );
        let response = app.form(&alice, Method::POST, &expiry, "expiry=keep").await;
        assert_eq!(response.status, StatusCode::OK);
        assert!(
            members(&app, folder)
                .await
                .contains(&("bob".into(), true, true, true))
        );
        let response = app.form(&alice, Method::POST, &expiry, "expiry=none").await;
        assert_eq!(response.status, StatusCode::OK);
        assert!(
            members(&app, folder)
                .await
                .contains(&("bob".into(), true, true, false))
        );
        let response = app
            .form(
                &alice,
                Method::POST,
                &expiry,
                "expiry=custom&expiry_date=2001-01-01",
            )
            .await;
        assert_eq!(response.status, StatusCode::BAD_REQUEST);
        assert_eq!(response.body, "Expiry must be in the future");

        // Members and strangers can't change anything: not found, nothing changed
        let before = members(&app, folder).await;
        for cookie in [&bob, &carol] {
            let responses = [
                app.form(cookie, Method::POST, &add, "user_id=carol&expiry=none")
                    .await,
                app.form(cookie, Method::POST, &permission, "permission=view")
                    .await,
                app.form(cookie, Method::POST, &expiry, "expiry=day").await,
                app.form(
                    cookie,
                    Method::DELETE,
                    &format!("/ui/sharing/share/{bob_share}"),
                    "",
                )
                .await,
            ];
            for response in responses {
                assert_eq!(response.status, StatusCode::NOT_FOUND, "{}", response.body);
            }
        }
        assert_eq!(members(&app, folder).await, before);

        // Removing
        let response = app
            .form(
                &alice,
                Method::DELETE,
                &format!("/ui/sharing/share/{bob_share}"),
                "",
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "{}", response.body);
        assert_eq!(triggers(&response)["fp:toast"]["message"], "Member removed");
        assert!(
            !members(&app, folder)
                .await
                .iter()
                .any(|(user, ..)| user == "bob")
        );

        // Public folders can't be shared with individuals
        let family = app.folder(None, "Trip").await;
        let response = app
            .form(
                &alice,
                Method::POST,
                &format!("/ui/sharing/folder/{family}/members"),
                "user_id=bob&expiry=none",
            )
            .await;
        assert_eq!(response.status, StatusCode::BAD_REQUEST);
    }
}
