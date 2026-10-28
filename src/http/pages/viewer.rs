//! The photo viewer's server side: the info panel fragment and the data of the viewer root
//! (`templates/viewer/root.html`, included by the shell). The viewer itself is
//! `assets/js/viewer/`.

use crate::http::AppStateRef;
use crate::http::api::photos_info::{PhotoInfo, load_photo_info};
use crate::http::auth::AuthenticatedUser;
use crate::http::error::HttpResult;
use crate::http::template_into_response::TemplateIntoResponse;
use crate::http::views::{DateStyle, format_bytes, format_date};
use crate::utils::exif::PhotoDetails;
use crate::utils::media::{BROWSER_IMAGE_EXTENSIONS, is_video};
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use serde::Deserialize;

/// The extensions of the originals browsers can show, for the viewer's zoom into the original
pub fn browser_image_extensions() -> String {
    BROWSER_IMAGE_EXTENSIONS.join(" ")
}

/// One row of the info panel's details card
struct DetailRow {
    icon: &'static str,
    title: String,
    summary: Option<String>,
    /// Makes the title a link
    href: Option<String>,
    /// Shows the title as a placeholder ("No folder")
    muted: bool,
    testid: &'static str,
}

struct LocationView {
    label: String,
    maps_url: String,
}

/// Everything the info panel shows, without the rows that have no data
struct InfoPanelView {
    photo_id: i64,
    name: String,
    date: String,
    location: Option<LocationView>,
    rows: Vec<DetailRow>,
}

#[derive(Template)]
#[template(path = "viewer/info_panel.html")]
struct InfoPanelTemplate {
    info: InfoPanelView,
    /// Rendered into `#app-dialog` (e.g. by the duplicates page) rather than the viewer's panel
    dialog: bool,
}

#[derive(Deserialize)]
pub struct InfoQuery {
    #[serde(default)]
    dialog: bool,
}

/// `GET /ui/viewer/info/{id}[?dialog=true]`: the info panel of a photo the user can read (else 404)
pub async fn info_panel(
    State(state): State<AppStateRef>,
    AuthenticatedUser(user): AuthenticatedUser,
    Path(photo_id): Path<i64>,
    Query(query): Query<InfoQuery>,
) -> HttpResult<Response> {
    let info = load_photo_info(state, &user.id, photo_id).await?;

    InfoPanelTemplate {
        info: info_panel_view(info, &user.id),
        dialog: query.dialog,
    }
    .try_into_response()
}

fn info_panel_view(info: PhotoInfo, user_id: &str) -> InfoPanelView {
    let PhotoInfo {
        photo,
        folder,
        owner,
        details,
        ..
    } = info;
    let mut rows = Vec::new();

    // 1. Type and resolution
    rows.push(DetailRow {
        icon: if is_video(&photo.name) {
            "videocam"
        } else {
            "image"
        },
        title: details.media_label.clone(),
        summary: resolution_line(&details),
        href: None,
        muted: false,
        testid: "viewer-info-type",
    });

    // 2. Camera and exposure
    if let Some((title, summary)) = camera_lines(&details) {
        rows.push(DetailRow {
            icon: "photo_camera",
            title,
            summary,
            href: None,
            muted: false,
            testid: "viewer-info-camera",
        });
    }

    // 3. Folder and size
    let size = (photo.file_size > 0).then(|| format_bytes(photo.file_size));
    rows.push(match folder {
        Some(folder) => DetailRow {
            icon: "folder",
            title: folder.name,
            summary: size,
            href: Some(format!("/folders/{}", folder.id)),
            muted: false,
            testid: "viewer-info-folder",
        },
        None => DetailRow {
            icon: "folder",
            title: "No folder".to_string(),
            summary: size,
            href: None,
            muted: true,
            testid: "viewer-info-folder",
        },
    });

    // 4. Visibility, with the owner of photos shared by someone else
    let visibility = match owner {
        None => ("public", "Family".to_string()),
        Some(owner) if owner.user_id == user_id => ("lock", "Private".to_string()),
        Some(owner) => (
            "lock",
            format!("Private · Shared by {}", owner.display_name),
        ),
    };
    rows.push(DetailRow {
        icon: visibility.0,
        title: "Visibility".to_string(),
        summary: Some(visibility.1),
        href: None,
        muted: false,
        testid: "viewer-info-visibility",
    });

    InfoPanelView {
        photo_id: photo.id,
        date: format_date(photo.created_at, DateStyle::Long),
        name: photo.name,
        location: details.gps.map(|gps| LocationView {
            label: format!("{:.4}, {:.4}", gps.lat, gps.lon),
            maps_url: format!(
                "https://www.openstreetmap.org/?mlat={lat:.5}&mlon={lon:.5}#map=15/{lat:.5}/{lon:.5}",
                lat = gps.lat,
                lon = gps.lon
            ),
        }),
        rows,
    }
}

/// `"4032 × 3024 (12.2 MP)"`, plus the duration of videos: `"320 × 240 (0.1 MP) · 00:02"`
fn resolution_line(details: &PhotoDetails) -> Option<String> {
    let resolution = match (details.width, details.height) {
        (Some(width), Some(height)) => {
            let megapixels = details
                .megapixels
                .map(|mp| format!(" ({mp:.1} MP)"))
                .unwrap_or_default();
            Some(format!("{width} × {height}{megapixels}"))
        }
        _ => None,
    };
    let duration = details.duration_secs.map(format_duration);

    match (resolution, duration) {
        (Some(resolution), Some(duration)) => Some(format!("{resolution} · {duration}")),
        (resolution, duration) => resolution.or(duration),
    }
}

/// `"mm:ss"`, or `"h:mm:ss"` from one hour, like the video controls
fn format_duration(seconds: f32) -> String {
    let total = seconds.max(0.0).round() as u64;
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

/// A number with at most one decimal: `1.8`, `2`
fn format_decimal(value: f32) -> String {
    let formatted = format!("{value:.1}");
    formatted
        .strip_suffix(".0")
        .map(str::to_string)
        .unwrap_or(formatted)
}

/// Like Android: `"Model (Lens)"` and `"f/1.8 · 1/250s · 26mm · ISO 100"`. The headline falls
/// back to the make, the lens, or "Camera" when only the settings are known.
fn camera_lines(details: &PhotoDetails) -> Option<(String, Option<String>)> {
    let camera = details
        .camera_model
        .clone()
        .or_else(|| details.camera_make.clone());
    let headline = match (camera, &details.lens_model) {
        (Some(camera), Some(lens)) => Some(format!("{camera} ({lens})")),
        (Some(camera), None) => Some(camera),
        (None, lens) => lens.clone(),
    };

    let settings: Vec<String> = [
        details.f_number.map(|f| format!("f/{}", format_decimal(f))),
        details.exposure.clone(),
        details
            .focal_length_mm
            .map(|mm| format!("{}mm", mm.round() as u32)),
        details.iso.map(|iso| format!("ISO {iso}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    let summary = (!settings.is_empty()).then(|| settings.join(" · "));

    match (headline, summary) {
        (None, None) => None,
        (headline, summary) => Some((headline.unwrap_or_else(|| "Camera".to_string()), summary)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::api::photos_info::FolderRef;
    use crate::http::api::tests::TestServer;
    use crate::model::photo::Photo;
    use crate::model::user::SimpleUser;
    use crate::repo::tests::{Dataset, create_test_folder, insert_photo, photo};
    use crate::utils::exif::GpsLocation;
    use axum::http::{HeaderName, StatusCode};
    use sqlx::SqlitePool;

    fn details() -> PhotoDetails {
        PhotoDetails {
            media_label: "Image (JPG)".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_resolution_line() {
        let cases = [
            (details(), None),
            (
                PhotoDetails {
                    width: Some(4032),
                    height: Some(3024),
                    megapixels: Some(12.2),
                    ..details()
                },
                Some("4032 × 3024 (12.2 MP)"),
            ),
            (
                PhotoDetails {
                    width: Some(320),
                    height: Some(240),
                    megapixels: Some(0.1),
                    duration_secs: Some(2.04),
                    ..details()
                },
                Some("320 × 240 (0.1 MP) · 00:02"),
            ),
            (
                PhotoDetails {
                    duration_secs: Some(3725.0),
                    ..details()
                },
                Some("1:02:05"),
            ),
        ];
        for (details, expected) in cases {
            assert_eq!(
                resolution_line(&details).as_deref(),
                expected,
                "{details:?}"
            );
        }
    }

    #[test]
    fn test_camera_lines() {
        let some = |s: &str| Some(s.to_string());
        let full = PhotoDetails {
            camera_make: some("Google"),
            camera_model: some("Pixel 8"),
            lens_model: some("Main camera"),
            f_number: Some(1.68),
            exposure: some("1/250s"),
            focal_length_mm: Some(6.9),
            iso: Some(100),
            ..details()
        };
        let cases = [
            (details(), None),
            (
                full.clone(),
                Some((
                    "Pixel 8 (Main camera)",
                    Some("f/1.7 · 1/250s · 7mm · ISO 100"),
                )),
            ),
            (
                PhotoDetails {
                    camera_model: None,
                    lens_model: None,
                    f_number: Some(2.0),
                    exposure: None,
                    focal_length_mm: None,
                    iso: None,
                    ..full.clone()
                },
                Some(("Google", Some("f/2"))),
            ),
            (
                PhotoDetails {
                    iso: Some(800),
                    ..details()
                },
                Some(("Camera", Some("ISO 800"))),
            ),
            (
                PhotoDetails {
                    lens_model: some("50mm"),
                    ..details()
                },
                Some(("50mm", None)),
            ),
        ];
        for (details, expected) in cases {
            let lines = camera_lines(&details);
            let lines = lines
                .as_ref()
                .map(|(title, summary)| (title.as_str(), summary.as_deref()));
            assert_eq!(lines, expected, "{details:?}");
        }
    }

    fn photo_info(photo: Photo, owner: Option<(&str, &str)>, details: PhotoDetails) -> PhotoInfo {
        PhotoInfo {
            photo,
            folder: None,
            owner: owner.map(|(user_id, display_name)| SimpleUser {
                user_id: user_id.to_string(),
                display_name: display_name.to_string(),
            }),
            is_favorite: false,
            can_delete: true,
            can_move: true,
            details,
        }
    }

    #[test]
    fn test_info_panel_view() {
        let view = info_panel_view(
            photo_info(
                Photo {
                    file_size: 1_258_291,
                    ..photo(Some("alice"), "a.jpg")
                },
                Some(("alice", "Alice")),
                PhotoDetails {
                    gps: Some(GpsLocation {
                        lat: 44.4268,
                        lon: -26.10256,
                    }),
                    ..details()
                },
            ),
            "alice",
        );
        assert_eq!(view.name, "a.jpg");
        assert_eq!(view.date, "Saturday, 1 June 2024 · 10:00");
        let location = view.location.unwrap();
        assert_eq!(location.label, "44.4268, -26.1026");
        assert_eq!(
            location.maps_url,
            "https://www.openstreetmap.org/?mlat=44.42680&mlon=-26.10256#map=15/44.42680/-26.10256"
        );
        let rows: Vec<_> = view
            .rows
            .iter()
            .map(|row| (row.testid, row.title.as_str(), row.summary.as_deref()))
            .collect();
        assert_eq!(
            rows,
            [
                ("viewer-info-type", "Image (JPG)", None),
                ("viewer-info-folder", "No folder", Some("1.2 MB")),
                ("viewer-info-visibility", "Visibility", Some("Private")),
            ]
        );

        // Visibility of public photos and of photos shared by someone else; folder links
        let cases = [
            (None, "Family", "public"),
            (Some(("bob", "Bob")), "Private · Shared by Bob", "lock"),
        ];
        for (owner, summary, icon) in cases {
            let info = PhotoInfo {
                folder: Some(FolderRef {
                    id: 3,
                    name: "Trip".to_string(),
                }),
                ..photo_info(photo(None, "clip.mp4"), owner, details())
            };
            let view = info_panel_view(info, "alice");
            assert_eq!(view.rows[0].icon, "videocam");
            assert_eq!(view.rows[1].title, "Trip");
            assert_eq!(view.rows[1].href.as_deref(), Some("/folders/3"));
            assert_eq!(view.rows[2].summary.as_deref(), Some(summary), "{owner:?}");
            assert_eq!(view.rows[2].icon, icon, "{owner:?}");
        }
    }

    /// A PNG header is enough for the resolution (read with `imagesize`)
    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        bytes
    }

    #[sqlx::test]
    async fn test_info_panel_route(pool: SqlitePool) {
        let server = TestServer::new(pool).await;
        let data = Dataset::create(server.pool()).await;
        let alice = server.login("alice").await;

        // Read access, like the info endpoint
        let cases = [
            ("own_1", StatusCode::OK),
            ("own_trashed", StatusCode::OK),
            ("public_a", StatusCode::OK),
            ("shared_1", StatusCode::OK),
            ("expired_1", StatusCode::NOT_FOUND),
            ("unshared_1", StatusCode::NOT_FOUND),
            ("carol_root", StatusCode::NOT_FOUND),
        ];
        for (name, status) in cases {
            let response = alice
                .get(&format!("/ui/viewer/info/{}", data.id(name)))
                .await;
            assert_eq!(response.status, status, "{name}");
            if status == StatusCode::OK {
                assert!(response.text().contains(name), "{name}");
            }
        }

        let shared = alice
            .get(&format!("/ui/viewer/info/{}", data.id("shared_1")))
            .await
            .text();
        assert!(shared.contains(r#"href="/folders/"#), "{shared}");
        assert!(shared.contains("Private · Shared by Bob"), "{shared}");

        // A real file: its resolution is read
        let png = insert_photo(server.pool(), photo(Some("alice"), "wide.png")).await;
        server.write_file(&png, None, &png_header(4032, 3024));
        let text = alice
            .get(&format!("/ui/viewer/info/{}", png.id))
            .await
            .assert_status(StatusCode::OK)
            .text();
        assert!(text.contains("Image (PNG)"), "{text}");
        assert!(text.contains("4032 × 3024 (12.2 MP)"), "{text}");
        assert!(!text.contains("app-dialog-title"), "{text}");

        // In a dialog: the heading labels it and there is a close button
        let text = alice
            .get(&format!("/ui/viewer/info/{}?dialog=true", png.id))
            .await
            .text();
        assert!(text.contains(r#"id="app-dialog-title""#), "{text}");
        assert!(text.contains("data-dialog-close"), "{text}");

        // Logged out: 401 with a redirect to the login page, back to the page
        let page = (
            HeaderName::from_static("hx-current-url"),
            "http://localhost/?photo=1",
        );
        let response = server
            .anonymous()
            .get_with(&format!("/ui/viewer/info/{}", png.id), &[page])
            .await
            .assert_status(StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.header(HeaderName::from_static("hx-redirect")),
            "/login?next=%2F%3Fphoto%3D1"
        );
    }

    /// The shell renders the viewer root with the deep link of the URL
    #[sqlx::test]
    async fn test_viewer_root_deep_link(pool: SqlitePool) {
        let server = TestServer::new(pool).await;
        let alice = server.login("alice").await;

        let cases = [
            (
                "/",
                r#"data-initial-photo="""#,
                r#"data-context="timeline:all""#,
            ),
            (
                "/?filter=family&photo=12",
                r#"data-initial-photo="12""#,
                r#"data-context="timeline:family""#,
            ),
            (
                "/?photo=12&ctx=memories:2",
                r#"data-initial-photo="12""#,
                r#"data-context="memories:2""#,
            ),
            (
                "/?photo=x&ctx=bogus",
                r#"data-initial-photo="""#,
                r#"data-context="timeline:all""#,
            ),
            // Pages without a grid leave the context to the viewer's script
            (
                "/utilities?photo=5",
                r#"data-initial-photo="5""#,
                r#"data-context="""#,
            ),
        ];
        let check = async |uri: &str, photo: &str, context: &str| {
            let text = alice.get(uri).await.assert_status(StatusCode::OK).text();
            assert!(text.contains(r#"data-testid="viewer""#), "{uri}");
            assert!(text.contains(photo), "{uri}: {photo}");
            assert!(text.contains(context), "{uri}: {context}");
            assert!(text.contains(r#"data-user-id="alice""#), "{uri}");
        };
        for (uri, photo, context) in cases {
            check(uri, photo, context).await;
        }

        // The folder page walks the folder, unless the URL names another context
        let folder = create_test_folder(server.pool(), Some("alice"), "Trip")
            .await
            .id;
        let folder_context = format!(r#"data-context="folder:{folder}""#);
        check(
            &format!("/folders/{folder}?photo=12"),
            r#"data-initial-photo="12""#,
            &folder_context,
        )
        .await;
        check(
            &format!("/folders/{folder}?photo=12&ctx=memories:2"),
            r#"data-initial-photo="12""#,
            r#"data-context="memories:2""#,
        )
        .await;

        // A deep link doesn't open the viewer over a 404 page
        for uri in ["/folders/999999?photo=12", "/nowhere?photo=12"] {
            let text = alice
                .get(uri)
                .await
                .assert_status(StatusCode::NOT_FOUND)
                .text();
            assert!(text.contains(r#"data-initial-photo="""#), "{uri}");
            assert!(text.contains(r#"data-context="""#), "{uri}");
        }
    }
}
