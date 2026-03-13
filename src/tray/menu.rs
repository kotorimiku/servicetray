use std::sync::{Arc, Mutex};

use rust_i18n::t;
use tray_icon::menu::{Icon, IconMenuItem, Menu, MenuItem, PredefinedMenuItem};

use crate::{config::AppConfig, event::MenuAction, process::ProcessManager};

/// Menu builder
pub struct MenuBuilder {
    action_map: Arc<Mutex<Vec<(String, MenuAction)>>>,
    process_manager: Arc<ProcessManager>,
}

impl MenuBuilder {
    /// Create a new menu builder
    pub fn new(
        action_map: Arc<Mutex<Vec<(String, MenuAction)>>>,
        process_manager: Arc<ProcessManager>,
    ) -> Self {
        MenuBuilder {
            action_map,
            process_manager,
        }
    }

    /// Build the tray menu
    pub fn build(&self, config: &AppConfig) -> Menu {
        // Clear old mappings
        self.action_map.lock().unwrap().clear();

        let tray_menu = Menu::new();

        // Add menu items for each program
        for program in &config.programs {
            // Service page link (if configured) with compact native icon
            if let Some(url) = &program.service_url {
                let is_running = self.process_manager.is_running(&program.name);
                let label = t!("open.link", name = program.name.clone());
                let url_item = IconMenuItem::new(label, true, build_status_icon(is_running), None);
                self.action_map
                    .lock()
                    .unwrap()
                    .push((url_item.id().0.clone(), MenuAction::OpenUrl(url.clone())));
                let _ = tray_menu.append(&url_item);
            }
        }

        // Separator
        let _ = tray_menu.append(&PredefinedMenuItem::separator());

        // Autostart toggle
        let autostart_label = if config.autostart {
            format!("✓ {}", t!("autostart"))
        } else {
            t!("autostart").to_string()
        };
        let autostart_item = MenuItem::new(autostart_label, true, None);
        self.action_map
            .lock()
            .unwrap()
            .push((autostart_item.id().0.clone(), MenuAction::ToggleAutostart));
        let _ = tray_menu.append(&autostart_item);

        // Open configuration file
        let open_config = MenuItem::new(t!("open.config"), true, None);
        self.action_map
            .lock()
            .unwrap()
            .push((open_config.id().0.clone(), MenuAction::OpenConfig));
        let _ = tray_menu.append(&open_config);

        // Exit
        let exit_item = MenuItem::new(t!("exit"), true, None);
        self.action_map
            .lock()
            .unwrap()
            .push((exit_item.id().0.clone(), MenuAction::Exit));
        let _ = tray_menu.append(&exit_item);

        tray_menu
    }
}

fn build_status_icon(is_running: bool) -> Option<Icon> {
    const ICON_SIZE: u32 = 12;
    const CENTER: f32 = 5.5;
    const RADIUS: f32 = 4.0;

    let color = if is_running {
        [34, 197, 94, 255]
    } else {
        [239, 68, 68, 255]
    };

    let mut rgba = vec![0_u8; (ICON_SIZE * ICON_SIZE * 4) as usize];
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let dx = x as f32 - CENTER;
            let dy = y as f32 - CENTER;
            if dx * dx + dy * dy <= RADIUS * RADIUS {
                let offset = ((y * ICON_SIZE + x) * 4) as usize;
                rgba[offset..offset + 4].copy_from_slice(&color);
            }
        }
    }

    Icon::from_rgba(rgba, ICON_SIZE, ICON_SIZE).ok()
}
