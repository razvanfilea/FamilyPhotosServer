use std::path::Path;

const VIDEO_EXTENSIONS: &[&str] = &[
    "3gp", "3g2", "mp4", "m4v", "mov", "mkv", "webm", "avi", "mpg", "mpeg", "ts", "mts", "m2ts",
    "flv", "wmv", "ogv",
];

const BROWSER_IMAGE_EXTENSIONS: &[&str] =
    &["jpg", "jpeg", "png", "gif", "webp", "avif", "bmp", "svg"];

fn extension(name: &str) -> Option<String> {
    Path::new(name)
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
}

fn has_extension(name: &str, extensions: &[&str]) -> bool {
    extension(name).is_some_and(|ext| extensions.contains(&ext.as_str()))
}

/// Whether the file is a video, judging by its extension (case-insensitive).
pub fn is_video(name: &str) -> bool {
    has_extension(name, VIDEO_EXTENSIONS)
}

/// Whether browsers can display the original file directly in an `<img>`.
pub fn is_browser_image(name: &str) -> bool {
    has_extension(name, BROWSER_IMAGE_EXTENSIONS)
}

/// Human-readable media type, e.g. "Image (JPG)" or "Video (MP4)".
pub fn media_label(name: &str) -> String {
    let kind = if is_video(name) { "Video" } else { "Image" };
    match extension(name).filter(|ext| !ext.is_empty()) {
        Some(ext) => format!("{kind} ({})", ext.to_ascii_uppercase()),
        None => kind.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_media_kind() {
        // (name, is video, is browser image, label)
        let cases = [
            ("clip.mp4", true, false, "Video (MP4)"),
            ("CLIP.MOV", true, false, "Video (MOV)"),
            ("a.b.MkV", true, false, "Video (MKV)"),
            ("old.3gp", true, false, "Video (3GP)"),
            ("IMG_1.jpg", false, true, "Image (JPG)"),
            ("a.JPG", false, true, "Image (JPG)"),
            ("a.webp", false, true, "Image (WEBP)"),
            ("a.svg", false, true, "Image (SVG)"),
            ("photo.heic", false, false, "Image (HEIC)"),
            ("a.dng", false, false, "Image (DNG)"),
            ("mp4", false, false, "Image"),
            ("noextension", false, false, "Image"),
            ("", false, false, "Image"),
        ];
        for (name, video, browser_image, label) in cases {
            assert_eq!(is_video(name), video, "{name}");
            assert_eq!(is_browser_image(name), browser_image, "{name}");
            assert_eq!(media_label(name), label, "{name}");
        }
    }
}
