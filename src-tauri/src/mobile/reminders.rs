use crate::commands::AppState;
use crate::error::{VaultError, VaultResult};
use chrono::{Duration, Local, TimeZone};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledReminder {
    pub key: String,
    pub entity_kind: String,
    pub entity_id: String,
    pub occurrence_key: String,
    pub slot: i64,
    pub title: String,
    pub body: String,
    pub at: String,
    pub when_ms: i64,
}

#[derive(Deserialize)]
struct Fired {
    key: String,
    entity_kind: String,
    entity_id: String,
    occurrence_key: String,
    slot: i64,
}

#[tauri::command]
pub async fn mobile_reminders_reconcile_cmd(app: tauri::AppHandle, state: State<'_, AppState>) -> VaultResult<serde_json::Value> {
    if !cfg!(target_os = "android") { return Ok(serde_json::json!({"supported":false})); }
    static SERIAL: std::sync::OnceLock<tauri::async_runtime::Mutex<()>> = std::sync::OnceLock::new();
    let _guard = SERIAL.get_or_init(|| tauri::async_runtime::Mutex::new(())).lock().await;
    let response = super::call(app.clone(), "drainReminderFires", serde_json::json!({})).await?;
    let fired: Vec<Fired> = serde_json::from_value(response["fires"].clone())
        .map_err(|_| VaultError::Validation("Invalid reminder delivery record".into()))?;
    state.inner.with_meta(|meta| {
        for item in &fired {
            meta.execute("INSERT OR IGNORE INTO reminder_fires(entity_kind,entity_id,occurrence_key,slot) VALUES(?1,?2,?3,?4)",
                rusqlite::params![item.entity_kind,item.entity_id,item.occurrence_key,item.slot])?;
        }
        Ok(())
    })?;
    let horizon = Local::now() + Duration::days(30);
    let (reminders, deferred) = state.inner.with_core_connections(|meta, tasks, calendar| {
        crate::remind::scheduled_plan(meta, tasks, calendar, horizon.naive_local())
    })?;
    let effective_horizon = if deferred > 0 { reminders.last().map(|r|r.when_ms).unwrap_or(horizon.timestamp_millis()).min(horizon.timestamp_millis()) } else { horizon.timestamp_millis() };
    let mut status = super::call(app.clone(), "scheduleReminders", serde_json::json!({"reminders":reminders,"horizon":effective_horizon})).await?;
    // Acknowledge native fire identities only after their SQLite records and the
    // replacement plan are durable. Interrupted reconciliation is repeatable.
    super::call(app, "ackReminderFires", serde_json::json!({"keys":fired.iter().map(|f| &f.key).collect::<Vec<_>>()})).await?;
    status["deferred"] = serde_json::json!(deferred);
    Ok(status)
}

#[tauri::command]
pub async fn mobile_alarm_settings_cmd(app: tauri::AppHandle) -> VaultResult<()> {
    super::call(app, "openAlarmSettings", serde_json::json!({})).await?;
    Ok(())
}

pub fn timestamp(value: chrono::NaiveDateTime) -> VaultResult<i64> {
    Local.from_local_datetime(&value).earliest().map(|t| t.timestamp_millis())
        .ok_or_else(|| VaultError::Validation("Reminder falls in a skipped local clock time".into()))
}
