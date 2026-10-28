use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Asset directories whose contents determine `ASSET_VERSION`
const VERSIONED_ASSET_DIRS: [&str; 4] =
    ["assets/css", "assets/js", "assets/vendor", "assets/icons"];

fn main() {
    // Always trigger recompilation when a new migration is added
    println!("cargo:rerun-if-changed=migrations");

    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());

    if profile == "release" {
        // In release mode, track the asset files and calculate a real hash
        let mut files = Vec::new();
        for dir in VERSIONED_ASSET_DIRS {
            println!("cargo:rerun-if-changed={dir}");
            collect_files(Path::new(dir), &mut files);
        }
        files.sort();

        let mut hasher = DefaultHasher::new();
        for file in &files {
            file.hash(&mut hasher);
            if let Ok(content) = fs::read(file) {
                content.hash(&mut hasher);
            }
        }
        println!("cargo:rustc-env=ASSET_VERSION={:x}", hasher.finish());
    } else {
        // In debug/dev mode, use a static string
        // DO NOT use rerun-if-changed on the assets here to avoid constant rebuilds
        println!("cargo:rustc-env=ASSET_VERSION=dev");
    }
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, files);
        } else {
            files.push(path);
        }
    }
}
