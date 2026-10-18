//! Native desktop entry points share the same `app-action` protocol as the React command panel.

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod single_instance;
pub mod window_state;
#[cfg(target_os = "linux")]
mod appindicator;

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Emitter, Manager,
};

use crate::commands::AppState;

pub struct DesktopCapabilities {
    tray_available: AtomicBool,
}

impl Default for DesktopCapabilities {
    fn default() -> Self {
        Self {
            tray_available: AtomicBool::new(false),
        }
    }
}

impl DesktopCapabilities {
    pub fn tray_available(&self) -> bool {
        self.tray_available.load(Ordering::Acquire)
    }
    fn set_tray_available(&self, value: bool) {
        self.tray_available.store(value, Ordering::Release);
    }
}

fn show_and_focus(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn emit_action(app: &tauri::AppHandle, action: &str) {
    show_and_focus(app);
    let _ = app.emit("app-action", action);
}

fn secure_quit(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        state.inner.session.lock_all();
        state.inner.stop_backup_scheduler();
    }
    app.exit(0);
}

/// Platform tray failures are intentionally non-fatal: the application keeps normal close
/// behavior on desktop environments without an available status area.
pub fn install(app: &tauri::AppHandle) {
    let result = (|| -> tauri::Result<()> {
        let toggle = MenuItem::with_id(app, "toggle", "显示/隐藏", true, None::<&str>)?;
        let note = MenuItem::with_id(app, "quick-note", "快速笔记", true, None::<&str>)?;
        let task = MenuItem::with_id(app, "quick-task", "快速任务", true, None::<&str>)?;
        let lock = MenuItem::with_id(app, "lock-all", "锁定全部", true, None::<&str>)?;
        let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
        let menu = Menu::with_items(app, &[&toggle, &note, &task, &lock, &quit])?;
        let mut tray = TrayIconBuilder::with_id("main-tray")
            .menu(&menu)
            .tooltip("Tenjee Vault")
            .show_menu_on_left_click(false)
            .on_menu_event(|app, event| match event.id.as_ref() {
                "toggle" => {
                    if let Some(window) = app.get_webview_window("main") {
                        if window.is_visible().unwrap_or(false) {
                            if let Some(exchange)=app.try_state::<crate::sync::session::ExchangeManager>(){exchange.cancel("App hidden; fresh pairing is required");}
                            let _ = window.hide();
                        } else {
                            show_and_focus(app);
                        }
                    }
                }
                "quick-note" => emit_action(app, "quick-note"),
                "quick-task" => emit_action(app, "quick-task"),
                "lock-all" => emit_action(app, "lock-all"),
                "quit" => secure_quit(app),
                _ => {}
            });
        if let Some(icon) = app.default_window_icon() {
            tray = tray.icon(icon.clone());
        }
        #[cfg(target_os = "linux")]
        let _appindicator_logs = appindicator::DeprecationNoticeGuard::new();
        tray.build(app)?;
        if let Some(capabilities) = app.try_state::<DesktopCapabilities>() {
            capabilities.set_tray_available(true);
        }
        Ok(())
    })();
    if let Err(error) = result {
        if let Some(capabilities) = app.try_state::<DesktopCapabilities>() {
            capabilities.set_tray_available(false);
        }
        eprintln!("system tray unavailable; using normal window lifecycle: {error}");
    }
}

pub fn quit(app: &tauri::AppHandle) {
    secure_quit(app);
}

#[tauri::command]
pub fn set_runtime_locale(app: tauri::AppHandle, state: tauri::State<'_,AppState>, locale: String) -> Result<(),crate::error::VaultError> {
    if !["zh-CN","en"].contains(&locale.as_str()) { return Err(crate::error::VaultError::Validation("Invalid locale".into())); }
    crate::commands::set_setting(&state.inner,"runtime_locale",&locale)?;
    let zh = locale == "zh-CN";
    if let Some(tray) = app.tray_by_id("main-tray") {
        let menu = Menu::new(&app).map_err(|e|crate::error::VaultError::Validation(e.to_string()))?;
        for (id,cn,en) in [("toggle","显示/隐藏","Show / hide"),("quick-note","快速笔记","Quick note"),("quick-task","快速任务","Quick task"),("lock-all","锁定全部","Lock all"),("quit","退出","Quit")] {
            let item = MenuItem::with_id(&app,id,if zh {cn} else {en},true,None::<&str>).map_err(|e|crate::error::VaultError::Validation(e.to_string()))?;
            menu.append(&item).map_err(|e|crate::error::VaultError::Validation(e.to_string()))?;
        }
        tray.set_menu(Some(menu)).map_err(|e|crate::error::VaultError::Validation(e.to_string()))?;
    }
    Ok(())
}
