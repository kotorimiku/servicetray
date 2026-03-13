#[derive(Debug)]
pub enum CustomEvent {
    ConfigUpdated(crate::config::AppConfig),
}

#[derive(Clone)]
pub enum MenuAction {
    OpenUrl(String),
    OpenConfig,
    ToggleAutostart,
    Exit,
}
