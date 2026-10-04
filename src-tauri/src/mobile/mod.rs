pub mod files;
pub mod reminders;

#[cfg(target_os = "android")]
pub struct NativeServices(pub tauri::plugin::PluginHandle<tauri::Wry>);

#[cfg(target_os = "android")]
pub fn init() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    use tauri::Manager;
    tauri::plugin::Builder::<tauri::Wry>::new("native-services")
        .setup(|app, api| {
            let handle = api.register_android_plugin("com.sam.tenjee_vault", "NativeServicesPlugin")?;
            app.manage(NativeServices(handle));
            Ok(())
        }).build()
}

pub async fn call(app: tauri::AppHandle, method: &'static str, args: serde_json::Value) -> crate::error::VaultResult<serde_json::Value> {
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;
        tauri::async_runtime::spawn_blocking(move || {
            app.state::<NativeServices>().0.run_mobile_plugin(method, args)
                .map_err(|error| crate::error::VaultError::Validation(format!("Android operation failed: {error}")))
        }).await.map_err(|_| crate::error::VaultError::Validation("Android operation interrupted".into()))?
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, method, args);
        Err(crate::error::VaultError::Validation("This operation requires Android".into()))
    }
}
