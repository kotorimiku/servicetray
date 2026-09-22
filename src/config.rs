use std::{env::home_dir, fs, path::PathBuf, sync::LazyLock};

use color_eyre::eyre::Result;
use serde::{Deserialize, Serialize};

const CONFIG_FILE_NAME: &str = "servicetray.json";
const PORTABLE_CONFIG_FILE_NAME: &str = "config.json";

static CONFIG_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    let exe_path = std::env::current_exe().unwrap_or(PathBuf::from("."));
    let default_dir = PathBuf::from(".");
    let exe_dir = exe_path.parent().unwrap_or(default_dir.as_path());

    let portable_config_path = exe_dir.join(PORTABLE_CONFIG_FILE_NAME);

    if portable_config_path.exists() {
        return portable_config_path;
    }

    let home = home_dir().unwrap_or(PathBuf::from("."));

    home.join(".config").join(CONFIG_FILE_NAME)
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProgramConfig {
    pub name: String,
    pub path: String,
    pub args: Option<Vec<String>>,
    pub service_url: Option<String>,
    #[serde(default)]
    pub watch_paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub programs: Vec<ProgramConfig>,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default)]
    pub log_level: Option<String>,
    #[serde(default)]
    pub save_log_file: bool,
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let config_path = Self::get_config_path();

        if !config_path.exists() {
            let default_config = Self::default();
            default_config.save()?;
            return Ok(default_config);
        }

        let content = fs::read_to_string(config_path)?;
        let config: AppConfig = serde_json::from_str(&content).unwrap_or(AppConfig::default());
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::get_config_path();

        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(self)?;
        fs::write(config_path, content)?;
        Ok(())
    }

    pub fn get_config_path() -> &'static PathBuf {
        &CONFIG_PATH
    }
}

/// Expands `~` or `~/...` (and `~\...` on Windows) to the user's home directory.
pub fn expand_tilde(path_str: &str) -> PathBuf {
    if path_str == "~" {
        return home_dir().unwrap_or_else(|| PathBuf::from("~"));
    }

    if let Some(rest) = path_str
        .strip_prefix("~/")
        .or_else(|| path_str.strip_prefix("~\\"))
        && let Some(home) = home_dir()
    {
        let mut path = home;
        for component in std::path::Path::new(rest).components() {
            path.push(component);
        }
        return path;
    }

    PathBuf::from(path_str)
}

/// Expands `~` within command arguments, supporting standalone paths (`~/...`)
/// and option assignments (`--option=~/...`).
pub fn expand_tilde_arg(arg: &str) -> String {
    if arg == "~" || arg.starts_with("~/") || arg.starts_with("~\\") {
        expand_tilde(arg).to_string_lossy().into_owned()
    } else if let Some((flag, val)) = arg.split_once('=') {
        if val == "~" || val.starts_with("~/") || val.starts_with("~\\") {
            format!("{flag}={}", expand_tilde(val).to_string_lossy())
        } else {
            arg.to_string()
        }
    } else {
        arg.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_program_config_watch_paths_deserialization() {
        let json = r#"{
            "name": "demo",
            "path": "demo.exe",
            "watch_paths": ["src", "config.toml"]
        }"#;
        let program: ProgramConfig = serde_json::from_str(json).unwrap();
        assert_eq!(
            program.watch_paths,
            Some(vec!["src".to_string(), "config.toml".to_string()])
        );

        let json_without_watch_paths = r#"{
            "name": "demo",
            "path": "demo.exe"
        }"#;
        let program_default: ProgramConfig =
            serde_json::from_str(json_without_watch_paths).unwrap();
        assert_eq!(program_default.watch_paths, None);
    }

    #[test]
    fn test_expand_tilde() {
        let home = home_dir().unwrap_or(PathBuf::from("."));
        assert_eq!(expand_tilde("~"), home);
        let expected = home.join("bin").join("app");
        assert_eq!(expand_tilde("~/bin/app"), expected);
        assert_eq!(expand_tilde("~\\bin\\app"), expected);
        assert_eq!(expand_tilde("custom_bin"), PathBuf::from("custom_bin"));
    }

    #[test]
    fn test_expand_tilde_arg() {
        let home = home_dir().unwrap_or(PathBuf::from("."));
        assert_eq!(expand_tilde_arg("--flag"), "--flag");
        let expected_path = home.join("data").to_string_lossy().into_owned();
        assert_eq!(expand_tilde_arg("~/data"), expected_path);
        let expected_opt = format!("--dir={expected_path}");
        assert_eq!(expand_tilde_arg("--dir=~/data"), expected_opt);
        assert_eq!(expand_tilde_arg("--foo=bar"), "--foo=bar");
    }
}
