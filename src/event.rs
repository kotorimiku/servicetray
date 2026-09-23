#[derive(Debug)]
pub enum CustomEvent {
    ConfigUpdated(crate::config::AppConfig),
    RestartProgramByWatcher(String),
    RefreshMenu,
}

#[derive(Clone)]
pub enum MenuAction {
    OpenUrl(String),
    OpenLog(std::path::PathBuf),
    RestartProgram(String),
    OpenConfig,
    ToggleAutostart,
    Exit,
}
