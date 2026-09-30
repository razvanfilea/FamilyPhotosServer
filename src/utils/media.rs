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
    fn test_is_video() {
        assert!(is_video("clip.mp4"));
        assert!(is_video("CLIP.MOV"));
        assert!(is_video("a.b.MkV"));
        assert!(is_video("old.3gp"));
        assert!(!is_video("photo.jpg"));
        assert!(!is_video("photo.heic"));
        assert!(!is_video("mp4"));
        assert!(!is_video(""));
    }

    #[test]
    fn test_is_browser_image() {
        assert!(is_browser_image("a.JPG"));
        assert!(is_browser_image("a.webp"));
        assert!(is_browser_image("a.svg"));
        assert!(!is_browser_image("a.heic"));
        assert!(!is_browser_image("a.dng"));
        assert!(!is_browser_image("a.mp4"));
    }

    #[test]
    fn test_media_label() {
        assert_eq!(media_label("IMG_1.jpg"), "Image (JPG)");
        assert_eq!(media_label("VID_1.Mp4"), "Video (MP4)");
        assert_eq!(media_label("photo.heic"), "Image (HEIC)");
        assert_eq!(media_label("noextension"), "Image");
    }
}
