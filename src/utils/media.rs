use std::path::Path;

const VIDEO_EXTENSIONS: &[&str] = &[
    "3gp", "3g2", "mp4", "m4v", "mov", "mkv", "webm", "avi", "mpg", "mpeg", "ts", "mts", "m2ts",
    "flv", "wmv", "ogv",
];

pub const BROWSER_IMAGE_EXTENSIONS: &[&str] =
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

/// GIF or WebP: formats that may be animated
pub fn is_gif_or_webp(name: &str) -> bool {
    has_extension(name, &["gif", "webp"])
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
        // (name, is video, is GIF or WebP, label)
        let cases = [
            ("clip.mp4", true, false, "Video (MP4)"),
            ("CLIP.MOV", true, false, "Video (MOV)"),
            ("a.b.MkV", true, false, "Video (MKV)"),
            ("old.3gp", true, false, "Video (3GP)"),
            ("IMG_1.jpg", false, false, "Image (JPG)"),
            ("a.JPG", false, false, "Image (JPG)"),
            ("a.webp", false, true, "Image (WEBP)"),
            ("anim.GIF", false, true, "Image (GIF)"),
            ("a.svg", false, false, "Image (SVG)"),
            ("photo.heic", false, false, "Image (HEIC)"),
            ("a.dng", false, false, "Image (DNG)"),
            ("mp4", false, false, "Image"),
            ("noextension", false, false, "Image"),
            ("", false, false, "Image"),
        ];
        for (name, video, gif_or_webp, label) in cases {
            assert_eq!(is_video(name), video, "{name}");
            assert_eq!(is_gif_or_webp(name), gif_or_webp, "{name}");
            assert_eq!(media_label(name), label, "{name}");
        }
    }
}
