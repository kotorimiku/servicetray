use std::{sync::RwLock, time::Duration};

use color_eyre::eyre::Result;
use crossbeam_channel::Sender;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use rust_i18n::t;
use tracing::info;

use crate::{config::AppConfig, event::CustomEvent};

/// Configuration file watcher
pub struct ConfigWatcher;

impl ConfigWatcher {
    /// Start the configuration file watcher
    pub fn start(
        config: std::sync::Arc<RwLock<AppConfig>>,
        event_sender: Sender<CustomEvent>,
    ) -> Result<RecommendedWatcher> {
        let config_path = AppConfig::get_config_path();

        let mut watcher =
            notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    // Only handle modify events for the config file
                    if event.kind.is_modify() {
                        // Sleep briefly to avoid multiple triggers when editor saves
                        std::thread::sleep(Duration::from_millis(100));

                        // Get the old configuration
                        let old_config = if let Ok(cfg) = config.read() {
                            cfg.clone()
                        } else {
                            return;
                        };

                        // Reload configuration
                        match AppConfig::load() {
                            Ok(new_config) => {
                                info!("{}", t!("config.file.updated.reloading"));

                                // Update shared configuration
                                if let Ok(mut cfg) = config.write() {
                                    *cfg = new_config.clone();
                                }

                                // Notify main thread that configuration was updated
                                let _ = event_sender.send(CustomEvent::ConfigUpdated(old_config));
                            }
                            Err(e) => {
                                tracing::error!(
                                    "{}",
                                    t!("config.reload.failed", error = e.to_string())
                                );
                            }
                        }
                    }
                }
            })?;

        // Watch the directory containing the config file (editors may delete/recreate the file)
        watcher.watch(config_path, RecursiveMode::NonRecursive)?;

        Ok(watcher)
    }
}
