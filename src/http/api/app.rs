use axum::{
    Router,
    extract::State,
    http::{HeaderMap, Method},
    response::Response,
    routing::get,
};

use crate::http::AppStateRef;
use crate::http::error::{HttpError, HttpResult};
use crate::http::utils::{FileResponseOptions, file_to_response};

/// Public: a new family member can install the app from a link, before having it
pub fn public_router() -> Router<AppStateRef> {
    Router::new().route("/app/download", get(download_apk))
}

/// `GET /api/app/download`: the APK of `APP_LATEST_VERSION` (the version announced by
/// `X-App-Latest-Version`), as `FamilyPhotos-<version>.apk`, with a `Content-Length` for the app's
/// progress bar. 404 when app updates aren't configured or the APK is missing.
///
/// `no-cache` with an `ETag`: a client always revalidates, so it can't keep an old APK once
/// `APP_APK_PATH` points at a new one.
async fn download_apk(
    State(state): State<AppStateRef>,
    method: Method,
    headers: HeaderMap,
) -> HttpResult<Response> {
    let release = state
        .available_app_release()
        .await
        .ok_or(HttpError::NotFound)?;

    let file_name = format!("FamilyPhotos-{}.apk", release.version);
    // The type comes from the `.apk` extension: `application/vnd.android.package-archive`
    file_to_response(
        &release.apk_path,
        &method,
        &headers,
        FileResponseOptions {
            cache_control: "no-cache",
            inline: false,
            file_name: Some(&file_name),
            validators: true,
        },
    )
    .await
}
