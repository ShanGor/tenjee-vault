//! Explain single-instance forwarding before the plugin exits the new process.

#[cfg(target_os = "linux")]
fn existing_instance_pid(identifier: &str) -> Option<u32> {
    let connection = zbus::blocking::Connection::session().ok()?;
    let reply = connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetConnectionUnixProcessID",
            &(format!("{identifier}.SingleInstance"),),
        )
        .ok()?;
    reply.body().deserialize().ok()
}

#[cfg(target_os = "windows")]
fn existing_instance_pid(identifier: &str) -> Option<u32> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId};

    // Match the hidden receiver window created by tauri-plugin-single-instance.
    let class_name: Vec<u16> = format!("{identifier}-sic")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let window_name: Vec<u16> = format!("{identifier}-siw")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // Both strings are NUL-terminated and live for the duration of the call.
    let window = unsafe { FindWindowW(class_name.as_ptr(), window_name.as_ptr()) };
    if window.is_null() {
        return None;
    }
    let mut pid = 0;
    // The window belongs to the existing app; the PID output points to valid local storage.
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    (pid != 0).then_some(pid)
}

pub fn report_existing_instance(identifier: &str) {
    if let Some(pid) = existing_instance_pid(identifier) {
        eprintln!(
            "Tenjee Vault is already running (PID {pid}). This launch will focus that process \
             and exit; the newly built app will not start. Use the existing app's Quit action, \
             then launch this build again."
        );
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::existing_instance_pid;

    #[test]
    fn detects_only_the_registered_single_instance_owner() {
        let identifier = format!("com.sam.tenjee_vault.test_{}", std::process::id());
        assert_eq!(existing_instance_pid(&identifier), None);

        let connection = zbus::blocking::connection::Builder::session()
            .unwrap()
            .name(format!("{identifier}.SingleInstance"))
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(existing_instance_pid(&identifier), Some(std::process::id()));

        connection
            .release_name(format!("{identifier}.SingleInstance"))
            .unwrap();
        assert_eq!(existing_instance_pid(&identifier), None);
    }
}

#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use super::existing_instance_pid;
    use windows_sys::Win32::{
        Foundation::HWND,
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, UnregisterClassW,
            WNDCLASSW,
        },
    };

    struct ReceiverWindow {
        window: HWND,
        class: WNDCLASSW,
        name: Vec<u16>,
    }

    impl Drop for ReceiverWindow {
        fn drop(&mut self) {
            // This test owns the window and its registered class.
            unsafe {
                if !self.window.is_null() {
                    DestroyWindow(self.window);
                }
                UnregisterClassW(self.name.as_ptr(), self.class.hInstance);
            }
        }
    }

    #[test]
    fn detects_only_the_plugins_hidden_receiver_window() {
        let identifier = format!("com.sam.tenjee_vault.test_{}", std::process::id());
        assert_eq!(existing_instance_pid(&identifier), None);
        let name: Vec<u16> = format!("{identifier}-sic")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let title: Vec<u16> = format!("{identifier}-siw")
            .encode_utf16()
            .chain(Some(0))
            .collect();

        // Zero is valid for the optional WNDCLASSW fields. The callback and strings remain valid
        // until the owning thread destroys the hidden window and unregisters the class.
        let mut class: WNDCLASSW = unsafe { std::mem::zeroed() };
        class.lpfnWndProc = Some(DefWindowProcW);
        class.hInstance = unsafe { GetModuleHandleW(std::ptr::null()) };
        class.lpszClassName = name.as_ptr();
        assert_ne!(unsafe { RegisterClassW(&class) }, 0);
        let mut receiver = ReceiverWindow {
            window: std::ptr::null_mut(),
            class,
            name,
        };
        receiver.window = unsafe {
            CreateWindowExW(
                0,
                receiver.name.as_ptr(),
                title.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                receiver.class.hInstance,
                std::ptr::null(),
            )
        };
        assert!(!receiver.window.is_null());
        assert_eq!(existing_instance_pid(&identifier), Some(std::process::id()));

        drop(receiver);
        assert_eq!(existing_instance_pid(&identifier), None);
    }
}
