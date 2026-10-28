//! Validation of user-supplied names that end up as paths on disk.

use std::path::Path;

pub const MAX_FOLDER_NAME_LENGTH: usize = 100;

/// Trims and validates a folder name, returning a user-facing message when it's not allowed.
/// Folders are directories directly under the user's directory, so the name must be a single,
/// non-hidden path component (a leading `.` could also collide with `.previews` or the DB).
pub fn validate_folder_name(name: &str) -> Result<String, &'static str> {
    let name = name.trim();

    if name.is_empty() {
        return Err("Folder name cannot be empty");
    }
    if name.contains(['/', '\\']) {
        return Err("Folder names can't contain / or \\");
    }
    if name.chars().any(char::is_control) {
        return Err("Folder names can't contain control characters");
    }
    if name.starts_with('.') {
        return Err("Folder names can't start with a dot");
    }
    if name.chars().count() > MAX_FOLDER_NAME_LENGTH {
        return Err("Folder names can't be longer than 100 characters");
    }

    Ok(name.to_owned())
}

/// Whether an uploaded file name is a single path component that can't escape its directory.
/// Uploads with other names are rejected before [`sanitize_file_name`] runs.
pub fn is_safe_path_component(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0'])
}

/// Makes an uploaded file name safe to store: keeps only the last path component and removes
/// control characters and slashes. Names that are empty, `.`/`..` or hidden are replaced with
/// `<uuid>.<ext>`.
pub fn sanitize_file_name(name: &str) -> String {
    let base_name = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = base_name
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_owned();

    if cleaned.is_empty() || cleaned.starts_with('.') {
        let extension = Path::new(&cleaned)
            .extension()
            .map(|ext| ext.to_string_lossy().to_string())
            .filter(|ext| !ext.is_empty());
        return match extension {
            Some(ext) => format!("{}.{ext}", uuid::Uuid::new_v4()),
            None => uuid::Uuid::new_v4().to_string(),
        };
    }

    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_folder_name() {
        let ok = [
            ("  Vacation 2024 ", "Vacation 2024"),
            ("Plajă", "Plajă"),
            ("a.b", "a.b"),
        ];
        for (name, expected) in ok {
            assert_eq!(
                validate_folder_name(name),
                Ok(expected.to_string()),
                "{name:?}"
            );
        }

        let err = [
            ("", "Folder name cannot be empty"),
            ("   ", "Folder name cannot be empty"),
            ("a/b", "Folder names can't contain / or \\"),
            ("a\\b", "Folder names can't contain / or \\"),
            ("/", "Folder names can't contain / or \\"),
            ("a\0b", "Folder names can't contain control characters"),
            ("a\nb", "Folder names can't contain control characters"),
            (".", "Folder names can't start with a dot"),
            ("..", "Folder names can't start with a dot"),
            (".previews", "Folder names can't start with a dot"),
        ];
        for (name, message) in err {
            assert_eq!(validate_folder_name(name), Err(message), "{name:?}");
        }

        // The length is in characters, not bytes
        for (name, valid) in [
            ("a".repeat(100), true),
            ("a".repeat(101), false),
            ("ă".repeat(100), true),
        ] {
            assert_eq!(validate_folder_name(&name).is_ok(), valid, "{}", name.len());
        }
    }

    #[test]
    fn test_sanitize_file_name() {
        assert_eq!(sanitize_file_name("IMG_1234.jpg"), "IMG_1234.jpg");
        assert_eq!(sanitize_file_name("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_file_name("C:\\Users\\me\\photo.png"), "photo.png");
        assert_eq!(sanitize_file_name("a\nb\0.jpg"), "ab.jpg");
        assert_eq!(sanitize_file_name(" spaced.jpg "), "spaced.jpg");

        for name in ["", "..", ".", "../..", "dir/", ".hidden.jpg", "\n"] {
            let sanitized = sanitize_file_name(name);
            assert!(!sanitized.starts_with('.'), "{name:?} → {sanitized}");
            assert!(!sanitized.is_empty());
            assert!(!sanitized.contains(['/', '\\']));
            assert!(uuid::Uuid::parse_str(sanitized.split('.').next().unwrap()).is_ok());
        }
        assert!(sanitize_file_name(".hidden.jpg").ends_with(".jpg"));
        assert!(!sanitize_file_name("..").contains('.'));
    }

    /// Names that could escape their parent directory are rejected both as folder names and
    /// as file names
    #[test]
    fn test_unsafe_path_components() {
        for name in ["vacation", "IMG_001.jpg"] {
            assert_eq!(validate_folder_name(name), Ok(name.to_string()));
            assert!(is_safe_path_component(name), "{name:?}");
            assert_eq!(sanitize_file_name(name), name);
        }

        for name in [
            "",
            ".",
            "..",
            "../bob",
            "/tmp/x",
            "a\\b",
            "a\0b",
            "../../etc/passwd",
        ] {
            assert!(validate_folder_name(name).is_err(), "{name:?}");
            assert!(!is_safe_path_component(name), "{name:?}");
        }

        // `..hidden` is a safe path component, but hidden names aren't allowed as folders and
        // get renamed as files
        assert!(is_safe_path_component("..hidden"));
        assert!(validate_folder_name("..hidden").is_err());
        assert!(!sanitize_file_name("..hidden").starts_with('.'));
    }
}
