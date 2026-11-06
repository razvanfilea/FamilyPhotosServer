use std::env::VarError;
use std::fmt::Display;
use std::path::PathBuf;
use std::str::FromStr;
use tracing::warn;

fn required_env_var(var_name: &str) -> Result<String, String> {
    std::env::var(var_name).map_err(|e| match e {
        VarError::NotPresent => format!("{var_name} must be set!"),
        VarError::NotUnicode(_) => format!("{var_name} is not valid Unicode!"),
    })
}

fn optional_env_var<V>(var_name: &str, default_value: V) -> V
where
    V: Copy + Display + FromStr,
    <V as FromStr>::Err: Display,
{
    std::env::var(var_name).map_or(default_value, |v| {
        v.parse::<V>()
            .inspect_err(|e| warn!("Failed to parse {var_name} defaulting to {default_value}: {e}"))
            .unwrap_or(default_value)
    })
}

pub struct EnvVariables {
    pub server_port: u16,
    pub storage_path: PathBuf,
    pub database_url: String,
    pub previews_path: PathBuf,
    pub scan_new_files: bool,
    pub background_threads_count: usize,
    pub allowed_origins: Vec<String>,
    pub app_release: Option<AppRelease>,
}

/// The latest Android app release, announced to the app and served by `/api/app/download`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRelease {
    /// The Android `versionCode`
    pub version: u32,
    pub apk_path: PathBuf,
}

impl AppRelease {
    /// Whether the APK can be served. Checked on every use, so the APK can be replaced (or put in
    /// place) without a restart, and users are never told to update to a file that is missing.
    pub async fn is_available(&self) -> bool {
        tokio::fs::metadata(&self.apk_path)
            .await
            .is_ok_and(|metadata| metadata.is_file())
    }

    /// Reads `APP_LATEST_VERSION` and `APP_APK_PATH`. `Err` when the config is set but incomplete
    /// or invalid; empty values count as unset.
    fn parse(version: Option<&str>, apk_path: Option<&str>) -> Result<Option<Self>, String> {
        let version = version.map(str::trim).filter(|v| !v.is_empty());
        let apk_path = apk_path.filter(|p| !p.trim().is_empty());

        match (version, apk_path) {
            (None, None) => Ok(None),
            (Some(_), None) => Err("APP_LATEST_VERSION is set but APP_APK_PATH isn't".to_string()),
            (None, Some(_)) => Err("APP_APK_PATH is set but APP_LATEST_VERSION isn't".to_string()),
            (Some(version), Some(apk_path)) => match version.parse::<u32>() {
                Ok(version) if version > 0 => Ok(Some(Self {
                    version,
                    apk_path: PathBuf::from(apk_path),
                })),
                _ => Err(format!(
                    "APP_LATEST_VERSION must be a positive integer, not {version:?}"
                )),
            },
        }
    }

    fn from_env() -> Option<Self> {
        let version = std::env::var("APP_LATEST_VERSION").ok();
        let apk_path = std::env::var("APP_APK_PATH").ok();
        let release = Self::parse(version.as_deref(), apk_path.as_deref())
            .inspect_err(|e| warn!("App updates disabled: {e}"))
            .ok()
            .flatten()?;

        if !release.apk_path.is_file() {
            warn!(
                "APP_APK_PATH {} is not a file: version {} won't be announced until it exists",
                release.apk_path.display(),
                release.version
            );
        }
        Some(release)
    }
}

impl EnvVariables {
    pub fn get_all() -> Result<Self, String> {
        dotenvy::dotenv().ok();

        let storage_path = PathBuf::from(required_env_var("STORAGE_PATH")?);
        if storage_path.exists() && !storage_path.is_dir() {
            return Err("STORAGE_PATH must be a directory!".to_string());
        }

        let previews_path = std::env::var("PREVIEWS_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| storage_path.join(".previews"));

        if previews_path.exists() && !previews_path.is_dir() {
            return Err("PREVIEWS_PATH must be a directory!".to_string());
        }

        let database_url = std::env::var("DATABASE_URL")
            .map(PathBuf::from)
            .unwrap_or_else(|_| storage_path.join(".familyphotos.db"));

        if database_url.exists() && !database_url.is_file() {
            return Err("DATABASE_URL must be a file!".to_string());
        }

        let allowed_origins = std::env::var("ALLOWED_ORIGINS")
            .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or_default();

        Ok(Self {
            server_port: required_env_var("SERVER_PORT")?
                .parse()
                .map_err(|_| "SERVER_PORT must be a valid port number!")?,
            storage_path,
            database_url: database_url.to_string_lossy().to_string(),
            previews_path,
            scan_new_files: optional_env_var("SCAN_NEW_FILES", true),
            background_threads_count: optional_env_var("BACKGROUND_THREADS_COUNT", 0),
            allowed_origins,
            app_release: AppRelease::from_env(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_app_release() {
        let release = |version| {
            Ok(Some(AppRelease {
                version,
                apk_path: PathBuf::from("/apk/app.apk"),
            }))
        };
        // (APP_LATEST_VERSION, APP_APK_PATH, expected: Ok(config) or Err)
        let cases = [
            (None, None, Ok(None)),
            (Some(""), Some(" "), Ok(None)),
            (Some("303"), Some("/apk/app.apk"), release(303)),
            (Some(" 1 "), Some("/apk/app.apk"), release(1)),
            (Some("303"), None, Err(())),
            (Some("303"), Some(""), Err(())),
            (None, Some("/apk/app.apk"), Err(())),
            (Some("0"), Some("/apk/app.apk"), Err(())),
            (Some("-3"), Some("/apk/app.apk"), Err(())),
            (Some("3.1"), Some("/apk/app.apk"), Err(())),
            (Some("v303"), Some("/apk/app.apk"), Err(())),
            (Some("99999999999"), Some("/apk/app.apk"), Err(())),
        ];
        for (version, path, expected) in cases {
            let actual = AppRelease::parse(version, path).map_err(|_| ());
            assert_eq!(actual, expected, "{version:?} {path:?}");
        }
    }
}
