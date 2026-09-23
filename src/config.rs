use std::{env::home_dir, fs, path::PathBuf, sync::LazyLock};

use color_eyre::eyre::Result;
use serde::{Deserialize, Serialize};

const CONFIG_FILE_NAME: &str = "servicetray.json";
const PORTABLE_CONFIG_FILE_NAME: &str = "config.json";

pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

static CONFIG_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    let exe_dir = exe_dir();
    let portable_config_path = exe_dir.join(PORTABLE_CONFIG_FILE_NAME);

    if portable_config_path.exists() {
        return portable_config_path;
    }

    let home = home_dir().unwrap_or(PathBuf::from("."));

    home.join(".config").join(CONFIG_FILE_NAME)
});

pub fn default_working_dir() -> String {
    "~".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProgramConfig {
    pub name: String,
    pub path: String,
    pub args: Option<Vec<String>>,
    pub service_url: Option<String>,
    #[serde(default)]
    pub watch_paths: Option<Vec<String>>,
    #[serde(default)]
    pub working_dir: Option<String>,
}

impl ProgramConfig {
    pub fn effective_working_dir<'a>(&'a self, global_working_dir: &'a str) -> &'a str {
        self.working_dir.as_deref().unwrap_or(global_working_dir)
    }

    pub fn log_path(&self) -> PathBuf {
        exe_dir().join("logs").join(format!("{}.log", self.name))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub programs: Vec<ProgramConfig>,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default)]
    pub log_level: Option<String>,
    #[serde(default)]
    pub save_log_file: bool,
    #[serde(default = "default_working_dir")]
    pub working_dir: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            programs: Vec::new(),
            autostart: false,
            log_level: None,
            save_log_file: false,
            working_dir: default_working_dir(),
        }
    }
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
    fn test_working_dir_priority_and_deserialization() {
        let app_json = r#"{
            "working_dir": "~/global",
            "programs": [
                { "name": "p1", "path": "p1.exe", "working_dir": "~/p1_cwd" },
                { "name": "p2", "path": "p2.exe" }
            ]
        }"#;
        let app_config: AppConfig = serde_json::from_str(app_json).unwrap();
        assert_eq!(app_config.working_dir, "~/global");
        assert_eq!(
            app_config.programs[0].working_dir,
            Some("~/p1_cwd".to_string())
        );
        assert_eq!(
            app_config.programs[0].effective_working_dir(&app_config.working_dir),
            "~/p1_cwd"
        );
        assert_eq!(app_config.programs[1].working_dir, None);
        assert_eq!(
            app_config.programs[1].effective_working_dir(&app_config.working_dir),
            "~/global"
        );

        let default_app = AppConfig::default();
        assert_eq!(default_app.working_dir, "~");

        let empty_json = "{}";
        let deserialized_default: AppConfig = serde_json::from_str(empty_json).unwrap();
        assert_eq!(deserialized_default.working_dir, "~");
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

    #[test]
    fn test_program_config_log_path() {
        let exe_dir = exe_dir();
        let prog = ProgramConfig {
            name: "test-app".to_string(),
            path: "test.exe".to_string(),
            args: None,
            service_url: None,
            watch_paths: None,
            working_dir: None,
        };
        let expected = exe_dir.join("logs").join("test-app.log");
        assert_eq!(prog.log_path(), expected);
    }
}
