use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};

fn main() {
    // Always trigger recompilation when a new migration is added
    println!("cargo:rerun-if-changed=migrations");

    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let css_path = "assets/css/main.css";

    if profile == "release" {
        // In release mode, track the CSS file and calculate a real hash
        println!("cargo:rerun-if-changed={}", css_path);

        let css_version = if let Ok(content) = fs::read(css_path) {
            let mut hasher = DefaultHasher::new();
            content.hash(&mut hasher);
            format!("{:x}", hasher.finish())
        } else {
            "release".to_string()
        };
        println!("cargo:rustc-env=CSS_VERSION={}", css_version);
    } else {
        // In debug/dev mode, use a static string
        // DO NOT use rerun-if-changed on the CSS file here to avoid constant rebuilds
        println!("cargo:rustc-env=CSS_VERSION=dev");
    }
}
