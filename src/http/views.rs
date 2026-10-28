//! View models and formatting helpers shared by all page templates.
//!
//! Dates follow the UTC "wall clock" convention: stored timestamps are shown as they are in UTC,
//! which is the time printed by the camera for EXIF photos.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Foundation for the grid and utility pages of later packages"
    )
)]

use crate::model::photo::Photo;
use crate::utils::media::is_video;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use time::{Month, OffsetDateTime};

/// Everything the `photo_tile` macro (`templates/macros/photo.html`) needs to render a tile
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhotoTileView {
    pub id: i64,
    pub is_video: bool,
    /// Whether the user has Delete access to the photo; gates the bulk actions
    pub deletable: bool,
    /// Standard base64 of the thumbhash, empty if the photo has none
    pub thumb_hash_b64: String,
    pub aria_label: String,
    pub created_at_unix: i64,
}

impl PhotoTileView {
    pub fn from_photo(photo: &Photo, deletable: bool) -> Self {
        let is_video = is_video(&photo.name);
        let kind = if is_video { "Video" } else { "Photo" };

        Self {
            id: photo.id,
            is_video,
            deletable,
            thumb_hash_b64: photo
                .thumb_hash
                .as_ref()
                .map(|hash| STANDARD.encode(hash))
                .unwrap_or_default(),
            aria_label: format!(
                "{kind}, {}",
                format_date(photo.created_at, DateStyle::Accessible)
            ),
            created_at_unix: photo.created_at.unix_timestamp(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateStyle {
    /// Month section header: `"June"` in the current year, `"June 2022"` otherwise
    MonthHeader,
    /// `"June 2024"`, e.g. in the month picker
    MonthWithYear,
    /// Viewer title: `"8 Jun 2024 · 14:03"`
    Viewer,
    /// Info panel: `"Saturday, 8 June 2024 · 14:03"`
    Long,
    /// Screen reader labels: `"8 Jun 2024 14:03"`
    Accessible,
}

pub fn format_date(date: OffsetDateTime, style: DateStyle) -> String {
    format_date_in_year(date, style, OffsetDateTime::now_utc().year())
}

fn format_date_in_year(date: OffsetDateTime, style: DateStyle, current_year: i32) -> String {
    let date = date.to_offset(time::UtcOffset::UTC);
    let month = date.month();
    let (hour, minute) = (date.hour(), date.minute());

    match style {
        DateStyle::MonthHeader if date.year() == current_year => month.to_string(),
        DateStyle::MonthHeader | DateStyle::MonthWithYear => format!("{month} {}", date.year()),
        DateStyle::Viewer => format!(
            "{} {} {} · {hour:02}:{minute:02}",
            date.day(),
            short_month(month),
            date.year()
        ),
        DateStyle::Long => format!(
            "{}, {} {month} {} · {hour:02}:{minute:02}",
            date.weekday(),
            date.day(),
            date.year()
        ),
        DateStyle::Accessible => format!(
            "{} {} {} {hour:02}:{minute:02}",
            date.day(),
            short_month(month),
            date.year()
        ),
    }
}

fn short_month(month: Month) -> &'static str {
    match month {
        Month::January => "Jan",
        Month::February => "Feb",
        Month::March => "Mar",
        Month::April => "Apr",
        Month::May => "May",
        Month::June => "Jun",
        Month::July => "Jul",
        Month::August => "Aug",
        Month::September => "Sep",
        Month::October => "Oct",
        Month::November => "Nov",
        Month::December => "Dec",
    }
}

/// Formats a size with 1024-based units like the Android app: `"512 B"`, `"1.2 MB"`, `"15 GB"`.
/// Must stay in sync with `formatBytes` in `assets/js/core/format.js`.
pub fn format_bytes(bytes: i64) -> String {
    const UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];

    if bytes.abs() < 1024 {
        return format!("{bytes} B");
    }

    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value.abs() >= 1023.5 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    // Values that would round up to 10 or 1024 are shown with the precision of the next step
    if value.abs() < 9.95 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

/// `"1 photo"` / `"3 photos"`. Must stay in sync with `plural` in `assets/js/core/format.js`.
pub fn plural(count: i64, singular: &str) -> String {
    if count == 1 {
        format!("{count} {singular}")
    } else {
        format!("{count} {singular}s")
    }
}

/// String equality for templates, where macro arguments and loop variables end up with different
/// levels of references (`&str` vs `&&str`) that `==` can't compare
pub fn str_eq(a: &str, b: &str) -> bool {
    a == b
}

/// The uppercase first letter of a name, for avatars
pub fn avatar_initial(name: &str) -> String {
    name.trim()
        .chars()
        .next()
        .map_or_else(|| "?".to_string(), |c| c.to_uppercase().collect())
}

/// A stable hue (0-359) derived from a name, so each user keeps the same avatar colour
pub fn avatar_hue(name: &str) -> u32 {
    // FNV-1a
    let hash = name.bytes().fold(0x811c9dc5u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x01000193)
    });
    hash % 360
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn test_format_date_styles() {
        let date = datetime!(2024-06-08 14:03:29 UTC);
        assert_eq!(
            format_date_in_year(date, DateStyle::MonthHeader, 2024),
            "June"
        );
        assert_eq!(
            format_date_in_year(date, DateStyle::MonthHeader, 2025),
            "June 2024"
        );
        assert_eq!(
            format_date_in_year(date, DateStyle::MonthWithYear, 2024),
            "June 2024"
        );
        assert_eq!(format_date(date, DateStyle::Viewer), "8 Jun 2024 · 14:03");
        assert_eq!(
            format_date(date, DateStyle::Long),
            "Saturday, 8 June 2024 · 14:03"
        );
        assert_eq!(format_date(date, DateStyle::Accessible), "8 Jun 2024 14:03");
    }

    #[test]
    fn test_format_date_uses_utc() {
        let date = datetime!(2024-01-01 01:30:00 +03:00);
        assert_eq!(format_date(date, DateStyle::Viewer), "31 Dec 2023 · 22:30");
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1024 * 512), "512 KB");
        assert_eq!(format_bytes(1_258_291), "1.2 MB");
        assert_eq!(format_bytes(15 * 1024 * 1024), "15 MB");
        assert_eq!(format_bytes(10 * 1024 * 1024 - 1), "10 MB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn test_plural() {
        assert_eq!(plural(0, "photo"), "0 photos");
        assert_eq!(plural(1, "photo"), "1 photo");
        assert_eq!(plural(2, "photo"), "2 photos");
    }

    /// Renders every shared macro, so their syntax and contracts are checked before they're used
    #[test]
    fn test_macros_render() {
        use askama::Template;

        #[derive(Template)]
        #[template(
            ext = "html",
            source = r#"{% import "macros/photo.html" as photo %}{% import "macros/ui.html" as ui %}{% import "macros/icons.html" as icons %}
{%- call photo::photo_tile(tile, 12) %}{% endcall %}
{%- call photo::photo_tile(tile, 13, "5 days") %}{% endcall %}
{%- call ui::filter_chips("family", "/") %}{% endcall %}
{%- call ui::empty_state("photo_library", "No photos yet", "") %}{% endcall %}
{%- call ui::badge("New", "primary") %}{% endcall %}
{%- call ui::avatar("Bob") %}{% endcall %}
{%- call ui::segmented("fview", [("grid", "Grid"), ("list", "List")], "list") %}{% endcall %}
{%- call icons::icon("star", true, "text-warning") %}{% endcall %}"#
        )]
        struct MacrosTemplate {
            tile: PhotoTileView,
        }

        let photo = Photo {
            id: 42,
            user_id: None,
            name: "clip.mp4".to_string(),
            created_at: datetime!(2024-06-08 14:03:29 UTC),
            file_size: 10,
            folder_id: None,
            thumb_hash: Some(vec![1, 2, 3]),
            trashed_on: None,
        };
        let html = MacrosTemplate {
            tile: PhotoTileView::from_photo(&photo, true),
        }
        .render()
        .unwrap();

        assert!(html.contains(r#"data-photo-id="42""#));
        assert!(html.contains(r#"data-index="12""#));
        assert!(html.contains(r#"data-video="true""#));
        assert!(html.contains(r#"data-deletable="true""#));
        assert!(html.contains(r#"data-thumbhash="AQID""#));
        assert!(html.contains(r#"aria-label="Video, 8 Jun 2024 14:03""#));
        assert!(html.contains(r#"src="/api/preview/42""#));
        assert!(html.contains("#play_circle-fill"));
        assert!(html.contains(r#"<span class="photo-tile__badge">5 days</span>"#));
        assert_eq!(html.matches("photo-tile__badge").count(), 1);
        assert!(html.contains(r#"data-testid="filter-chip-family" aria-pressed="true""#));
        assert!(html.contains(r#"data-testid="filter-chip-all" aria-pressed="false""#));
        assert!(html.contains(r#"data-href="/?filter=shared""#));
        assert!(html.contains("No photos yet"));
        assert!(!html.contains("fp-empty__body"));
        assert!(html.contains("badge-primary"));
        assert!(html.contains(&format!("--avatar-hue: {}", avatar_hue("Bob"))));
        assert!(html.contains(
            r#"value="list" aria-label="List"
           data-testid="fview-list" checked"#
        ));
        assert!(html.contains(r#"<svg class="icon text-warning""#));
        assert!(html.contains("#star-fill"));
    }

    #[test]
    fn test_avatar() {
        assert_eq!(avatar_initial(" alice"), "A");
        assert_eq!(avatar_initial("ștefan"), "Ș");
        assert_eq!(avatar_initial(""), "?");
        assert_eq!(avatar_hue("Alice"), avatar_hue("Alice"));
        assert!(avatar_hue("Bob") < 360);
    }

    #[test]
    fn test_photo_tile_view() {
        let photo = Photo {
            id: 7,
            user_id: None,
            name: "clip.MP4".to_string(),
            created_at: datetime!(2024-06-08 14:03:29 UTC),
            file_size: 10,
            folder_id: None,
            thumb_hash: Some(vec![1, 2, 3]),
            trashed_on: None,
        };

        let tile = PhotoTileView::from_photo(&photo, true);
        assert_eq!(tile.id, 7);
        assert!(tile.is_video);
        assert!(tile.deletable);
        assert_eq!(tile.thumb_hash_b64, "AQID");
        assert_eq!(tile.aria_label, "Video, 8 Jun 2024 14:03");
        assert_eq!(tile.created_at_unix, 1717855409);

        let photo = Photo {
            name: "a.jpg".to_string(),
            thumb_hash: None,
            ..photo
        };
        let tile = PhotoTileView::from_photo(&photo, false);
        assert!(!tile.is_video);
        assert!(tile.thumb_hash_b64.is_empty());
        assert_eq!(tile.aria_label, "Photo, 8 Jun 2024 14:03");
    }
}
