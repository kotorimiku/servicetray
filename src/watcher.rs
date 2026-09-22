use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use color_eyre::eyre::Result;
use crossbeam_channel::{Receiver, Sender, unbounded};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use rust_i18n::t;
use tracing::info;

use crate::{
    config::{AppConfig, ProgramConfig, expand_tilde},
    event::CustomEvent,
};
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

/// Resolves a watch path against base dir if relative.
fn resolve_watch_path(path_str: &str, base_dir: &Path) -> PathBuf {
    let path = expand_tilde(path_str);
    if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    }
}

/// Program file/directory watcher manager with debounced restart triggers.
pub struct ProgramWatcherManager {
    base_dir: PathBuf,
    watchers: HashMap<String, (Vec<String>, RecommendedWatcher)>,
    debounce_tx: Sender<String>,
    running: Arc<AtomicBool>,
    debounce_thread: Option<std::thread::JoinHandle<()>>,
}

impl ProgramWatcherManager {
    pub fn new(event_sender: Sender<CustomEvent>) -> Result<Self> {
        let config_path = AppConfig::get_config_path();
        let base_dir = config_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        Self::with_base_dir(event_sender, base_dir)
    }

    pub fn with_base_dir(event_sender: Sender<CustomEvent>, base_dir: PathBuf) -> Result<Self> {
        let (debounce_tx, debounce_rx) = unbounded::<String>();
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();
        let event_sender_clone = event_sender;

        let debounce_thread = std::thread::spawn(move || {
            Self::debounce_loop(debounce_rx, event_sender_clone, running_clone);
        });

        Ok(Self {
            base_dir,
            watchers: HashMap::new(),
            debounce_tx,
            running,
            debounce_thread: Some(debounce_thread),
        })
    }

    fn debounce_loop(rx: Receiver<String>, tx: Sender<CustomEvent>, running: Arc<AtomicBool>) {
        let mut pending: HashMap<String, Instant> = HashMap::new();
        let debounce_duration = Duration::from_millis(300);

        while running.load(Ordering::SeqCst) {
            let timeout = if let Some(min_deadline) =
                pending.values().map(|t| *t + debounce_duration).min()
            {
                let now = Instant::now();
                if min_deadline > now {
                    min_deadline - now
                } else {
                    Duration::from_millis(0)
                }
            } else {
                Duration::from_millis(100)
            };

            match rx.recv_timeout(timeout) {
                Ok(name) => {
                    pending.insert(name, Instant::now());
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }

            let now = Instant::now();
            let mut triggered = Vec::new();
            pending.retain(|name, time| {
                if now.duration_since(*time) >= debounce_duration {
                    triggered.push(name.clone());
                    false
                } else {
                    true
                }
            });

            for name in triggered {
                let _ = tx.send(CustomEvent::RestartProgramByWatcher(name));
            }
        }
    }

    pub fn update(&mut self, programs: &[ProgramConfig]) {
        let desired: HashMap<&str, &Vec<String>> = programs
            .iter()
            .filter_map(|p| {
                p.watch_paths
                    .as_ref()
                    .filter(|paths| !paths.is_empty())
                    .map(|paths| (p.name.as_str(), paths))
            })
            .collect();

        // Remove obsolete or updated watchers
        self.watchers.retain(|name, (current_paths, _)| {
            if let Some(new_paths) = desired.get(name.as_str()) {
                current_paths == *new_paths
            } else {
                false
            }
        });

        // Add missing watchers
        for (name, paths) in desired {
            if self.watchers.contains_key(name) {
                continue;
            }

            let name_str = name.to_string();
            let debounce_tx = self.debounce_tx.clone();
            let watcher_res =
                notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                    if let Ok(event) = res
                        && !event.kind.is_access()
                    {
                        let _ = debounce_tx.send(name_str.clone());
                    }
                });

            let mut watcher = match watcher_res {
                Ok(w) => w,
                Err(e) => {
                    tracing::error!(
                        "{}",
                        t!(
                            "watch.path.failed",
                            path = "watcher_init",
                            name = name,
                            error = e.to_string()
                        )
                    );
                    continue;
                }
            };

            let mut watched_any = false;
            for path_str in paths {
                let path = resolve_watch_path(path_str, &self.base_dir);
                if !path.exists() {
                    tracing::error!(
                        "{}",
                        t!(
                            "watch.path.not_found",
                            path = path.display().to_string(),
                            name = name
                        )
                    );
                    continue;
                }

                let mode = if path.is_dir() {
                    RecursiveMode::Recursive
                } else {
                    RecursiveMode::NonRecursive
                };

                if let Err(e) = watcher.watch(&path, mode) {
                    tracing::error!(
                        "{}",
                        t!(
                            "watch.path.failed",
                            path = path.display().to_string(),
                            name = name,
                            error = e.to_string()
                        )
                    );
                } else {
                    watched_any = true;
                }
            }

            if watched_any {
                self.watchers
                    .insert(name.to_string(), (paths.clone(), watcher));
            }
        }
    }
}

impl Drop for ProgramWatcherManager {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        // Clear watchers first so notify stops producing events
        self.watchers.clear();
        if let Some(handle) = self.debounce_thread.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn test_program_watcher_debounce_triggers_once() {
        let unique_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("servicetray_test_{unique_id}"));
        fs::create_dir_all(&temp_dir).unwrap();
        let file_path = temp_dir.join("watch_me.txt");
        fs::write(&file_path, "initial").unwrap();

        let (event_sender, event_receiver) = unbounded();
        let mut manager =
            ProgramWatcherManager::with_base_dir(event_sender, temp_dir.clone()).unwrap();
        let program = ProgramConfig {
            name: "test-prog".to_string(),
            path: "cmd.exe".to_string(),
            args: None,
            service_url: None,
            watch_paths: Some(vec!["watch_me.txt".to_string()]),
        };

        manager.update(&[program]);

        // Trigger multiple rapid writes
        for i in 0..5 {
            fs::write(&file_path, format!("update {}", i)).unwrap();
            std::thread::sleep(Duration::from_millis(50));
        }

        // Wait for debounce window (300ms + buffer)
        let received = event_receiver.recv_timeout(Duration::from_millis(1500));
        assert!(
            matches!(&received, Ok(CustomEvent::RestartProgramByWatcher(name)) if name == "test-prog"),
            "Expected restart event, got {:?}",
            received
        );
        // Assert no duplicate events immediately following
        let duplicate = event_receiver.recv_timeout(Duration::from_millis(200));
        drop(manager);
        let _ = fs::remove_dir_all(&temp_dir);
        assert!(
            duplicate.is_err(),
            "Unexpected extra event received: {:?}",
            duplicate
        );
    }

    #[test]
    fn test_resolve_watch_path() {
        let home = std::env::home_dir().unwrap_or(PathBuf::from("."));
        let base = Path::new("/base/dir");
        assert_eq!(
            resolve_watch_path("~/my_watch", base),
            home.join("my_watch")
        );
        assert_eq!(
            resolve_watch_path("relative_watch", base),
            base.join("relative_watch")
        );
    }
}
