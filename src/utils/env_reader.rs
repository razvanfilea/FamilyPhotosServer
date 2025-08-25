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
            return Err("DATABASE_URL must be a directory!".to_string());
        }

        Ok(Self {
            server_port: required_env_var("SERVER_PORT")?
                .parse()
                .map_err(|_| "SERVER_PORT must be a valid port number!")?,
            storage_path,
            database_url: database_url.to_string_lossy().to_string(),
            previews_path,
            scan_new_files: optional_env_var("SCAN_NEW_FILES", true),
            background_threads_count: optional_env_var("BACKGROUND_THREADS_COUNT", 0),
        })
    }
}
