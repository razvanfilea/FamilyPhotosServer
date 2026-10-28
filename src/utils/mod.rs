pub mod env_reader;
pub mod exif;
pub mod password_hash;
pub mod storage_resolver;

const BLAKE_3_LEN: usize = 32;

pub fn crop_blake_3_hash(hash: &[u8; BLAKE_3_LEN]) -> Vec<u8> {
    hash[..BLAKE_3_LEN / 2].to_vec()
}

/// Returns true if `name` can be safely used as a single path component
/// (a folder or file name) without escaping its parent directory.
pub fn is_safe_path_component(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', '\0'])
}

#[cfg(test)]
mod tests {
    use super::is_safe_path_component;

    #[test]
    fn test_safe_path_component() {
        assert!(is_safe_path_component("vacation"));
        assert!(is_safe_path_component("IMG_001.jpg"));
        assert!(is_safe_path_component("..hidden"));

        assert!(!is_safe_path_component(""));
        assert!(!is_safe_path_component("."));
        assert!(!is_safe_path_component(".."));
        assert!(!is_safe_path_component("../bob"));
        assert!(!is_safe_path_component("/tmp/x"));
        assert!(!is_safe_path_component("a\\b"));
        assert!(!is_safe_path_component("a\0b"));
    }
}
