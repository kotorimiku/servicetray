#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use rust_i18n::t;

rust_i18n::i18n!("locales", fallback = "en");

pub mod autostart;
pub mod cli;
pub mod config;
pub mod event;
pub mod log;
pub mod process;
pub mod tray;
pub mod watcher;

use color_eyre::eyre::Result;
use tracing::info;

use crate::{config::AppConfig, tray::TrayApp};

#[cfg(windows)]
fn attach_console() {
    use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};

    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;
    #[cfg(windows)]
    attach_console();

    let args = cli::Args::parse();
    let config = AppConfig::load()?;

    let log_level = args.log_level.as_deref().or(config.log_level.as_deref());
    let save_log_file = args.save_log_file.unwrap_or(config.save_log_file);

    // initialize logging and keep the guard alive for the program lifetime
    let _log_guard = log::init(log_level, save_log_file)?;

    let locale = sys_locale::get_locale().unwrap_or_else(|| String::from("en-US"));

    rust_i18n::set_locale(&locale);

    let app = TrayApp::new(config);

    app.run()?;

    info!("{}", t!("tray.app.exited"));

    Ok(())
}
