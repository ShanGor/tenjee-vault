use super::{destination, manifest::invalid};
use crate::error::VaultResult;
use serde_json::Value;
use std::path::Path;

#[cfg(target_os = "android")]
static APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();
pub fn init(app: &tauri::AppHandle) {
    #[cfg(target_os = "android")]
    {
        let _ = APP.set(app.clone());
    }
    #[cfg(not(target_os = "android"))]
    let _ = app;
}
pub fn provider(value: &str) -> bool {
    value.starts_with("content://")
}
pub fn call(method: &'static str, args: Value) -> VaultResult<Value> {
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;
        APP.get()
            .ok_or_else(|| invalid("Android file services are unavailable"))?
            .state::<crate::mobile::NativeServices>()
            .0
            .run_mobile_plugin(method, args)
            .map_err(|e| invalid(format!("Android file operation failed: {e}")))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (method, args);
        Err(invalid("Document provider access requires Android"))
    }
}
pub fn open_source(value: &str) -> VaultResult<cap_std::fs::File> {
    if !provider(value) {
        return destination::source(Path::new(value));
    }
    #[cfg(target_os = "android")]
    {
        let result = call("openTransferSource", serde_json::json!({"source":value}))?;
        return descriptor(&result);
    }
    #[cfg(not(target_os = "android"))]
    Err(invalid("Document provider access requires Android"))
}
#[cfg(target_os = "android")]
pub fn descriptor(result: &Value) -> VaultResult<cap_std::fs::File> {
    use std::os::fd::FromRawFd;
    let fd = result["fd"]
        .as_i64()
        .filter(|v| *v >= 0 && *v <= i32::MAX as i64)
        .ok_or_else(|| invalid("Invalid native source descriptor"))? as i32;
    // The native adapter transfers ownership using ParcelFileDescriptor.detachFd.
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    Ok(cap_std::fs::File::from_std(file))
}
pub fn available_space(value: &str) -> VaultResult<u64> {
    if !provider(value) {
        return Ok(fs2::available_space(value)?);
    }
    // Providers need not expose capacity. Unknown capacity is disclosed during
    // picking; writes still enforce app-private staging space and native errors.
    Ok(u64::MAX)
}
pub fn destination_identity(value: &str) -> VaultResult<destination::Identity> {
    if provider(value) {
        let response = call(
            "transferDestinationIdentity",
            serde_json::json!({"uri":value}),
        )?;
        super::manifest::decode(&super::manifest::bytes(&response["identity"])?)
    } else {
        Ok(destination::identity(
            &destination::absolute_dir(Path::new(value))?.dir_metadata()?,
        ))
    }
}
