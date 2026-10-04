//! Compatibility commands for desktop capabilities unavailable on mobile.
use crate::commands::AppState;
use tauri::Manager;

#[derive(Default)]
pub struct DesktopCapabilities;
impl DesktopCapabilities {
    pub fn tray_available(&self) -> bool {
        false
    }
}

pub fn replace_system_shortcuts(_: &tauri::AppHandle, _: &str, _: &str) -> Result<(), String> {
    // The app's keyboard map still works with an attached keyboard. OS-global
    // shortcut registration is a desktop capability and is not attempted here.
    Ok(())
}

pub fn quit(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        state.inner.session.lock_all();
        state.inner.stop_backup_scheduler();
    }
    app.exit(0);
}

#[tauri::command]
pub fn set_runtime_locale(
    _app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    locale: String,
) -> Result<(), crate::error::VaultError> {
    if !["zh-CN", "en"].contains(&locale.as_str()) {
        return Err(crate::error::VaultError::Validation(
            "Invalid locale".into(),
        ));
    }
    crate::commands::set_setting(&state.inner, "runtime_locale", &locale)
}
