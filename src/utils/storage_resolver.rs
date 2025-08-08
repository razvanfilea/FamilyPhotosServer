use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct StorageResolver {
    pub storage_folder: PathBuf,
    pub preview_folder: PathBuf,
}

impl StorageResolver {
    pub fn new(storage_folder: PathBuf, preview_folder: PathBuf) -> StorageResolver {
        if !storage_folder.exists() {
            fs::create_dir_all(&storage_folder).unwrap_or_else(|_| {
                panic!(
                    "Failed to create storage folder at {}",
                    storage_folder.display()
                )
            });
        }

        if !preview_folder.exists() {
            fs::create_dir_all(&preview_folder).unwrap_or_else(|_| {
                panic!(
                    "Failed to create preview folder at {}",
                    preview_folder.display()
                )
            });
        }

        StorageResolver {
            storage_folder,
            preview_folder,
        }
    }

    pub fn resolve_photo<P: AsRef<Path>>(&self, relative: P) -> PathBuf {
        self.storage_folder.join(relative.as_ref())
    }

    pub fn resolve_preview<P: AsRef<Path>>(&self, relative: P) -> PathBuf {
        self.preview_folder.join(relative.as_ref())
    }

    pub fn move_photo<P1: AsRef<Path>, P2: AsRef<Path>>(
        &self,
        src_relative: P1,
        dest_relative: P2,
    ) -> std::io::Result<()> {
        let destination_path = self.resolve_photo(dest_relative);
        if destination_path.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "Photo already exists",
            ));
        }

        // Create a parent directory if it doesn't exist
        if let Some(parent) = destination_path.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent)?;
        }

        fs::rename(self.resolve_photo(src_relative), destination_path)
    }
}
