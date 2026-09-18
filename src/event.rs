#[derive(Debug)]
pub enum CustomEvent {
    ConfigUpdated(crate::config::AppConfig),
    RestartProgramByWatcher(String),
    RefreshMenu,
}

#[derive(Clone)]
pub enum MenuAction {
    OpenUrl(String),
    RestartProgram(String),
    OpenConfig,
    ToggleAutostart,
    Exit,
}
