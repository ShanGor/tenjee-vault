//! Validate persisted dimensions before asking the window-state plugin to resize GTK windows.

use tauri::{plugin::TauriPlugin, AppHandle, Manager};
use tauri_plugin_window_state::{AppHandleExt, StateFlags, WindowExt};

const FLAGS: StateFlags = StateFlags::POSITION.union(StateFlags::SIZE);

pub fn init() -> TauriPlugin<tauri::Wry> {
    tauri_plugin_window_state::Builder::default()
        .with_state_flags(FLAGS)
        // The plugin accepts zero dimensions on restore. Restore main ourselves after
        // checking the saved size, while retaining its move/resize tracking and saving.
        .skip_initial_state("main")
        .build()
}

pub fn restore(app: &AppHandle) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    let saved = std::fs::read(app.path().app_config_dir()?.join(app.filename()))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    window.restore_state(restore_flags(saved.as_ref(), window.scale_factor()?))
}

fn restore_flags(saved: Option<&serde_json::Value>, scale_factor: f64) -> StateFlags {
    let valid_size = saved
        .and_then(|states| states.get("main"))
        .is_some_and(|state| {
            ["width", "height"].iter().all(|key| {
                state
                    .get(key)
                    .and_then(serde_json::Value::as_u64)
                    .is_some_and(|size| {
                        // GTK consumes positive signed logical dimensions, whereas the
                        // plugin stores unsigned physical dimensions.
                        size > 0
                            && size <= i32::MAX as u64
                            && (size as f64 / scale_factor).round() >= 1.0
                    })
            })
        });
    if valid_size {
        FLAGS
    } else {
        // Keep the configured default size if the saved state is absent or unusable.
        StateFlags::POSITION
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn restores_valid_physical_sizes() {
        let saved = json!({"main": {"width": 2200, "height": 1440}});
        assert_eq!(restore_flags(Some(&saved), 2.0).bits(), FLAGS.bits());
    }

    #[test]
    fn rejects_dimensions_that_cannot_resize_gtk() {
        for (width, height) in [(0, 0), (0, 720), (1100, 0), (u32::MAX, 720)] {
            let saved = json!({"main": {"width": width, "height": height}});
            assert_eq!(
                restore_flags(Some(&saved), 1.0).bits(),
                StateFlags::POSITION.bits()
            );
        }
        let saved = json!({"main": {"width": 1, "height": 720}});
        assert_eq!(
            restore_flags(Some(&saved), 3.0).bits(),
            StateFlags::POSITION.bits()
        );
    }

    #[test]
    fn missing_or_malformed_dimensions_keep_the_default_size() {
        assert_eq!(restore_flags(None, 1.0).bits(), StateFlags::POSITION.bits());
        for saved in [
            json!({}),
            json!({"main": {}}),
            json!({"main": {"width": -1, "height": 720}}),
            json!({"main": {"width": "1100", "height": 720}}),
        ] {
            assert_eq!(
                restore_flags(Some(&saved), 1.0).bits(),
                StateFlags::POSITION.bits()
            );
        }
    }
}
