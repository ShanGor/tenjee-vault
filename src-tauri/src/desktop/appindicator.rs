//! Compatibility with Ayatana 0.5.94's subsequently withdrawn deprecation notice.

const DOMAIN: &str = "libayatana-appindicator";
const DEPRECATION_NOTICE: &str = "libayatana-appindicator is deprecated. Please use libayatana-appindicator-glib in newly written code.";

pub(super) struct DeprecationNoticeGuard(Option<glib::LogHandlerId>);

impl DeprecationNoticeGuard {
    pub(super) fn new() -> Self {
        // Tauri's GTK tray still requires the GTK/dbusmenu ABI; the GLib replacement
        // is not a drop-in substitute. Upstream withdrew this notice in 0.6.0:
        // https://github.com/AyatanaIndicators/libayatana-appindicator/releases/tag/0.6.0
        // Filter only this exact notice during tray creation on older installations.
        Self(Some(glib::log_set_handler(
            Some(DOMAIN),
            glib::LogLevels::LEVEL_WARNING,
            false,
            false,
            |domain, level, message| {
                if message != DEPRECATION_NOTICE {
                    glib::log_default_handler(domain, level, Some(message));
                }
            },
        )))
    }
}

impl Drop for DeprecationNoticeGuard {
    fn drop(&mut self) {
        if let Some(handler) = self.0.take() {
            glib::log_remove_handler(Some(DOMAIN), handler);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_only_the_notice_during_tray_creation() {
        // Exercise GLib in a subprocess so diagnostics and process-global log handlers
        // cannot interfere with the rest of the test suite.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "desktop::appindicator::tests::emit_compatibility_logs",
                "--ignored",
                "--nocapture",
            ])
            .env_remove("G_DEBUG")
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stderr}");
        assert_eq!(stderr.matches(DEPRECATION_NOTICE).count(), 1, "{stderr}");
        assert!(stderr.contains("other tray warning"), "{stderr}");
        assert!(stderr.contains("other GTK warning"), "{stderr}");
    }

    #[test]
    #[ignore = "subprocess fixture for filters_only_the_notice_during_tray_creation"]
    fn emit_compatibility_logs() {
        {
            let _guard = DeprecationNoticeGuard::new();
            glib::g_warning!(DOMAIN, "{DEPRECATION_NOTICE}");
            glib::g_warning!(DOMAIN, "other tray warning");
            glib::g_warning!("Gtk", "other GTK warning");
        }
        glib::g_warning!(DOMAIN, "{DEPRECATION_NOTICE}");
    }
}
