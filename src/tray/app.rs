use std::sync::{
    Arc, Mutex, RwLock,
    atomic::{AtomicBool, Ordering},
};

use color_eyre::eyre::{Result, eyre};
use crossbeam_channel::unbounded;
use rust_i18n::t;
use tracing::info;
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use winit::{application::ApplicationHandler, event_loop::EventLoop};

use crate::{
    config::AppConfig,
    event::{CustomEvent, MenuAction},
    process::ProcessManager,
    tray::menu::MenuBuilder,
    watcher::{ConfigWatcher, ProgramWatcherManager},
};
fn create_icon() -> Result<Icon> {
    let icon_bytes = include_bytes!("../../assets/icon.png");
    let img = image::load_from_memory(icon_bytes)
        .map_err(|e| eyre!("{}", t!("load.icon.failed", error = e.to_string())))?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    Icon::from_rgba(rgba.into_raw(), width, height)
        .map_err(|e| eyre!("{}", t!("create.icon.failed", error = e.to_string())))
}

pub struct TrayApp {
    config: Arc<RwLock<AppConfig>>,
    process_manager: Arc<ProcessManager>,
}

impl TrayApp {
    pub fn new(config: AppConfig) -> Self {
        TrayApp {
            config: Arc::new(RwLock::new(config)),
            process_manager: Arc::new(ProcessManager::new()),
        }
    }

    fn start_all_processes(&self) {
        let config = self.config.read().unwrap();
        for program in &config.programs {
            if let Err(e) = self.process_manager.start(program, &config.working_dir) {
                tracing::error!(
                    "{}",
                    t!(
                        "start.program.failed",
                        name = &program.name,
                        error = e.to_string()
                    )
                );
            }
        }
    }

    pub fn run(self) -> Result<()> {
        let event_loop = EventLoop::with_user_event().build()?;
        let running = Arc::new(AtomicBool::new(true));

        let action_map: Arc<Mutex<Vec<(String, MenuAction)>>> = Arc::new(Mutex::new(Vec::new()));

        self.start_all_processes();

        let config = self.config.read().unwrap();
        let menu_builder = MenuBuilder::new(action_map.clone(), self.process_manager.clone());
        let tray_menu = menu_builder.build(&config);

        let icon = create_icon()?;
        let tooltip_label = t!("service.tray").to_string();
        let tray = TrayIconBuilder::new()
            .with_icon(icon)
            .with_menu(Box::new(tray_menu))
            .with_tooltip(tooltip_label.as_str())
            .build()?;

        let (event_sender, event_receiver) = unbounded();

        let _watcher = ConfigWatcher::start(self.config.clone(), event_sender.clone())?;
        let mut program_watcher = ProgramWatcherManager::new(event_sender)?;
        program_watcher.update(&config.programs);
        drop(config);

        let event_loop_proxy = event_loop.create_proxy();

        let process_manager = self.process_manager.clone();
        let action_map_clone = action_map.clone();
        let running_clone = running.clone();
        let config_clone = self.config.clone();
        let process_manager_for_update = self.process_manager.clone();

        let menu_event_receiver = tray_icon::menu::MenuEvent::receiver();

        std::thread::spawn(move || {
            loop {
                crossbeam_channel::select! {
                    recv(menu_event_receiver) -> event => {
                        let Ok(event) = event else { continue };
                        let action = {
                            let actions = action_map_clone.lock().unwrap();
                            actions
                                .iter()
                                .find(|(id, _)| id == &event.id.0)
                                .map(|(_, a)| a.clone())
                        };
                        if let Some(action) = action {
                            Self::handle_menu_action(
                                &action,
                                &process_manager,
                                &config_clone,
                                &running_clone,
                                &event_loop_proxy,
                            );
                        }
                    }
                    recv(event_receiver) -> event => {
                        match event {
                            Ok(CustomEvent::ConfigUpdated(old_config)) => {
                                if let Ok(new_config) = config_clone.read() {
                                    info!("{}", t!("config.reloaded.count", count = new_config.programs.len()));
                                    Self::update_processes_internal(
                                        &process_manager_for_update,
                                        &old_config,
                                        &new_config,
                                    );
                                    program_watcher.update(&new_config.programs);
                                    let _ = event_loop_proxy.send_event(CustomEvent::ConfigUpdated(old_config));
                                }
                            }
                            Ok(CustomEvent::RestartProgramByWatcher(name)) => {
                                info!("{}", t!("program.file.changed.restarting", name = &name));
                                if Self::restart_or_start_program(
                                    &process_manager_for_update,
                                    &config_clone,
                                    &name,
                                ) {
                                    let _ = event_loop_proxy.send_event(CustomEvent::RefreshMenu);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        // Run the event loop
        let mut app_handler = AppHandler {
            running,
            tray: Some(tray),
            config: self.config.clone(),
            action_map,
            process_manager: self.process_manager.clone(),
        };
        event_loop.run_app(&mut app_handler)?;

        Ok(())
    }

    fn restart_program(
        process_manager: &ProcessManager,
        config: &RwLock<AppConfig>,
        name: &str,
    ) -> bool {
        Self::restart_or_start_program(process_manager, config, name)
    }

    fn restart_or_start_program(
        process_manager: &ProcessManager,
        config: &RwLock<AppConfig>,
        name: &str,
    ) -> bool {
        let Ok(cfg) = config.read() else { return false };
        let Some(program) = cfg.programs.iter().find(|p| p.name == name) else {
            return false;
        };

        if process_manager.is_running(&program.name) {
            info!("{}", t!("restarting.program", name = &program.name));
            if let Err(e) = process_manager.stop(&program.name) {
                tracing::error!(
                    "{}",
                    t!(
                        "failed.to.stop.program",
                        name = &program.name,
                        error = e.to_string()
                    )
                );
                return false;
            }
        }

        if let Err(e) = process_manager.start(program, &cfg.working_dir) {
            tracing::error!(
                "{}",
                t!(
                    "start.program.failed",
                    name = &program.name,
                    error = e.to_string()
                )
            );
        }
        true
    }

    fn handle_menu_action(
        action: &MenuAction,
        process_manager: &ProcessManager,
        config: &RwLock<AppConfig>,
        running: &AtomicBool,
        proxy: &winit::event_loop::EventLoopProxy<CustomEvent>,
    ) {
        match action {
            MenuAction::OpenUrl(url) => {
                if let Err(e) = open::that_detached(url) {
                    tracing::error!("{}", t!("open.url.failed", error = e.to_string()));
                }
            }
            MenuAction::OpenLog(path) => {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if !path.exists() {
                    let _ = std::fs::File::create(path);
                }
                if let Err(e) = open::that_detached(path) {
                    tracing::error!("{}", t!("open.log.failed", error = e.to_string()));
                }
            }
            MenuAction::RestartProgram(name) => {
                if Self::restart_program(process_manager, config, name) {
                    let _ = proxy.send_event(CustomEvent::RefreshMenu);
                }
            }
            MenuAction::ToggleAutostart => {
                let (autostart, old_cfg) = {
                    let Ok(mut cfg) = config.write() else { return };
                    let old_cfg = cfg.clone();
                    cfg.autostart = !cfg.autostart;
                    if let Err(e) = cfg.save() {
                        tracing::error!("{}", t!("open.config.failed", error = e.to_string()));
                    }
                    (cfg.autostart, old_cfg)
                };

                if let Err(e) = crate::autostart::set_autostart(autostart) {
                    tracing::error!("{}", t!("tray.app.error", error = e.to_string()));
                }

                let _ = proxy.send_event(CustomEvent::ConfigUpdated(old_cfg));
            }
            MenuAction::OpenConfig => {
                let config_path = AppConfig::get_config_path();
                if let Err(e) = open::that_detached(config_path) {
                    tracing::error!("{}", t!("open.config.failed", error = e.to_string()));
                }
            }
            MenuAction::Exit => {
                process_manager.stop_all();
                running.store(false, Ordering::SeqCst);
                std::process::exit(0);
            }
        }
    }

    /// Start/stop processes based on configuration changes
    fn update_processes_internal(
        process_manager: &ProcessManager,
        old_config: &AppConfig,
        new_config: &AppConfig,
    ) {
        use std::collections::HashSet;

        let old_names: HashSet<_> = old_config.programs.iter().map(|p| &p.name).collect();
        let new_names: HashSet<_> = new_config.programs.iter().map(|p| &p.name).collect();

        // Stop processes that were removed
        for name in old_names.difference(&new_names) {
            if let Err(e) = process_manager.stop(name) {
                tracing::error!(
                    "{}",
                    t!("failed.to.stop.program", name = name, error = e.to_string())
                );
            }
        }

        // Start newly added processes
        for program in &new_config.programs {
            if !old_names.contains(&program.name)
                && let Err(e) = process_manager.start(program, &new_config.working_dir)
            {
                tracing::error!(
                    "{}",
                    t!(
                        "start.program.failed",
                        name = &program.name,
                        error = e.to_string()
                    )
                );
            }
        }

        // Restart processes with changed properties
        for new_program in &new_config.programs {
            let Some(old_program) = old_config
                .programs
                .iter()
                .find(|p| p.name == new_program.name)
            else {
                continue;
            };

            let old_cwd = old_program.effective_working_dir(&old_config.working_dir);
            let new_cwd = new_program.effective_working_dir(&new_config.working_dir);
            if old_program == new_program && old_cwd == new_cwd {
                continue;
            }

            info!(
                "{}",
                t!(
                    "program.config.changed.restarting",
                    name = &new_program.name
                )
            );
            if let Err(e) = process_manager.stop(&new_program.name) {
                tracing::error!(
                    "{}",
                    t!(
                        "failed.to.stop.program",
                        name = &new_program.name,
                        error = e.to_string()
                    )
                );
            }
            if let Err(e) = process_manager.start(new_program, &new_config.working_dir) {
                tracing::error!(
                    "{}",
                    t!(
                        "start.program.failed",
                        name = &new_program.name,
                        error = e.to_string()
                    )
                );
            }
        }
    }
}

/// Application event handler
struct AppHandler {
    running: Arc<AtomicBool>,
    tray: Option<TrayIcon>,
    config: Arc<RwLock<AppConfig>>,
    action_map: Arc<Mutex<Vec<(String, MenuAction)>>>,
    process_manager: Arc<ProcessManager>,
}

impl ApplicationHandler<CustomEvent> for AppHandler {
    fn resumed(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {}

    fn window_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        _event: winit::event::WindowEvent,
    ) {
    }

    fn user_event(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop, event: CustomEvent) {
        match event {
            CustomEvent::ConfigUpdated(_) | CustomEvent::RefreshMenu => {
                // Update menu in main thread
                if let Ok(config) = self.config.read() {
                    let menu_builder =
                        MenuBuilder::new(self.action_map.clone(), self.process_manager.clone());
                    let new_menu = menu_builder.build(&config);
                    if let Some(tray) = &self.tray {
                        tray.set_menu(Some(Box::new(new_menu)));
                    }
                    info!("{}", t!("tray.menu.updated"));
                }
            }
            CustomEvent::RestartProgramByWatcher(_) => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if !self.running.load(Ordering::SeqCst) {
            event_loop.exit();
        }
    }
}
