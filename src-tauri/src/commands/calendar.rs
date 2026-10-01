//! Thin Tauri bridge for the calendar domain.

use chrono::NaiveDate;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::State;

use super::AppState;
use crate::calendar::events::{self, DayInfo, Event, EventInstance, EventLinkState, EventReminder};
use crate::error::{VaultError, VaultResult};
use crate::portability::file_boundary::SafeDestination;
use crate::search::EventSearchHit;

#[derive(Debug, Deserialize)]
pub struct NewEventInput {
    pub title: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start_at: String,
    pub end_at: String,
    #[serde(default)]
    pub all_day: bool,
    pub timezone: Option<String>,
    pub recurrence_rule: Option<String>,
    pub lunar_recurrence: Option<String>,
    pub color: Option<String>,
    pub linked_page_ref: Option<String>,
    pub linked_task_id: Option<String>,
    #[serde(default)]
    pub reminders: Vec<i64>,
}

#[derive(Debug, Deserialize)]
pub struct EventPatchInput {
    pub title: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub clear_description: bool,
    pub location: Option<String>,
    #[serde(default)]
    pub clear_location: bool,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
    pub all_day: Option<bool>,
    pub timezone: Option<String>,
    #[serde(default)]
    pub clear_timezone: bool,
    pub recurrence_rule: Option<String>,
    #[serde(default)]
    pub clear_recurrence_rule: bool,
    pub lunar_recurrence: Option<String>,
    #[serde(default)]
    pub clear_lunar_recurrence: bool,
    pub color: Option<String>,
    #[serde(default)]
    pub clear_color: bool,
    pub linked_page_ref: Option<String>,
    #[serde(default)]
    pub clear_linked_page_ref: bool,
    pub linked_task_id: Option<String>,
    #[serde(default)]
    pub clear_linked_task_id: bool,
}

impl From<EventPatchInput> for events::EventPatch {
    fn from(value: EventPatchInput) -> Self {
        Self {
            title: value.title,
            description: value
                .description
                .map(Some)
                .or(value.clear_description.then_some(None)),
            location: value
                .location
                .map(Some)
                .or(value.clear_location.then_some(None)),
            start_at: value.start_at,
            end_at: value.end_at,
            all_day: value.all_day,
            timezone: value
                .timezone
                .map(Some)
                .or(value.clear_timezone.then_some(None)),
            recurrence_rule: value
                .recurrence_rule
                .map(Some)
                .or(value.clear_recurrence_rule.then_some(None)),
            lunar_recurrence: value
                .lunar_recurrence
                .map(Some)
                .or(value.clear_lunar_recurrence.then_some(None)),
            color: value.color.map(Some).or(value.clear_color.then_some(None)),
            linked_page_ref: value
                .linked_page_ref
                .map(Some)
                .or(value.clear_linked_page_ref.then_some(None)),
            linked_task_id: value
                .linked_task_id
                .map(Some)
                .or(value.clear_linked_task_id.then_some(None)),
        }
    }
}

fn date(value: &str) -> VaultResult<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| VaultError::Validation(format!("日期 {value} 非法")))
}

#[derive(Debug, Clone, Serialize)]
pub struct CalendarImportResult {
    pub created: usize,
    pub updated: usize,
    pub copied: usize,
    pub warnings: Vec<String>,
}

fn selected_ical_path(path: &str) -> VaultResult<&Path> {
    let path = Path::new(path);
    if !path.is_absolute() || !path.is_file() {
        return Err(VaultError::Validation(
            "导入路径必须是文件对话框选择的绝对常规文件".into(),
        ));
    }
    Ok(path)
}

fn conflict_policy(value: &str) -> VaultResult<events::IcalConflictPolicy> {
    match value {
        "update" => Ok(events::IcalConflictPolicy::Update),
        "copy" => Ok(events::IcalConflictPolicy::Copy),
        _ => Err(VaultError::Validation(
            "日程冲突策略必须为 update 或 copy".into(),
        )),
    }
}

/// Parse first, then use one calendar transaction. This makes malformed mixed files report
/// warnings/errors without leaving a partially-created calendar behind.
#[tauri::command]
pub fn import_calendar_ical_cmd(
    state: State<'_, AppState>,
    path: String,
    conflict: String,
) -> Result<CalendarImportResult, VaultError> {
    let bytes = std::fs::read(selected_ical_path(&path)?)?;
    let source = std::str::from_utf8(&bytes)
        .map_err(|_| VaultError::Validation("iCalendar 文件必须是 UTF-8".into()))?;
    let plan = crate::portability::ical::parse(source)?;
    let warnings = plan.warnings.clone();
    let result = state
        .inner
        .with_calendar(|conn| events::import_ical_plan(conn, &plan, conflict_policy(&conflict)?))?;
    Ok(CalendarImportResult {
        created: result.created,
        updated: result.updated,
        copied: result.copied,
        warnings,
    })
}

/// Writes the already-rendered calendar through the shared safe-output boundary so an existing
/// `.ics` is never touched until a complete temporary file is ready to publish.
#[tauri::command]
pub fn export_calendar_ical_cmd(
    state: State<'_, AppState>,
    ids: Option<Vec<String>>,
    range_start: Option<String>,
    range_end: Option<String>,
    directory: String,
    filename: String,
    overwrite: bool,
) -> Result<(), VaultError> {
    let range = match (range_start, range_end) {
        (Some(start), Some(end)) => Some((date(&start)?, date(&end)?)),
        (None, None) => None,
        _ => {
            return Err(VaultError::Validation(
                "日程导出范围必须同时提供开始和结束日期".into(),
            ));
        }
    };
    let content = state
        .inner
        .with_calendar(|conn| events::export_ical(conn, ids.as_deref(), range))?;
    SafeDestination::in_selected_directory(Path::new(&directory), &filename)?
        .write_atomic(content.as_bytes(), overwrite)
}

#[tauri::command]
pub fn create_event_cmd(
    state: State<'_, AppState>,
    input: NewEventInput,
) -> Result<Event, VaultError> {
    state.inner.with_calendar(|conn| {
        let event = events::create_event(
            conn,
            &input.title,
            input.description.as_deref(),
            input.location.as_deref(),
            &input.start_at,
            &input.end_at,
            input.all_day,
            input.timezone.as_deref(),
            input.recurrence_rule.as_deref(),
            input.lunar_recurrence.as_deref(),
            input.color.as_deref(),
            input.linked_page_ref.as_deref(),
            input.linked_task_id.as_deref(),
        )?;
        if let Err(error) = events::set_reminders(conn, &event.id, &input.reminders) {
            let _ = events::delete_event(conn, &event.id);
            return Err(error);
        }
        Ok(event)
    })
}

#[tauri::command]
pub fn get_event_cmd(state: State<'_, AppState>, id: String) -> Result<Event, VaultError> {
    state
        .inner
        .with_calendar(|conn| events::get_event(conn, &id))
}

#[tauri::command]
pub fn update_event_cmd(
    state: State<'_, AppState>,
    id: String,
    patch: EventPatchInput,
) -> Result<Event, VaultError> {
    state
        .inner
        .with_calendar(|conn| events::update_event(conn, &id, patch.into()))
}

#[tauri::command]
pub fn delete_event_cmd(state: State<'_, AppState>, id: String) -> Result<(), VaultError> {
    state
        .inner
        .with_calendar(|conn| events::delete_event(conn, &id))
}

#[tauri::command]
pub fn calendar_instances(
    state: State<'_, AppState>,
    range_start: String,
    range_end: String,
) -> Result<Vec<EventInstance>, VaultError> {
    let start = date(&range_start)?;
    let end = date(&range_end)?;
    if end < start {
        return Err(VaultError::Validation(
            "范围结束日期不得早于开始日期".into(),
        ));
    }
    state
        .inner
        .with_calendar(|conn| events::instances_in_range(conn, start, end))
}

#[tauri::command]
pub fn move_event_instance(
    state: State<'_, AppState>,
    event_id: String,
    original_start_at: String,
    new_start_at: String,
    new_end_at: String,
) -> Result<(), VaultError> {
    state.inner.with_calendar(|conn| {
        events::move_instance(
            conn,
            &event_id,
            &original_start_at,
            &new_start_at,
            &new_end_at,
        )
    })
}

#[tauri::command]
pub fn cancel_event_instance(
    state: State<'_, AppState>,
    event_id: String,
    original_start_at: String,
) -> Result<(), VaultError> {
    state
        .inner
        .with_calendar(|conn| events::cancel_instance(conn, &event_id, &original_start_at))
}

#[tauri::command]
pub fn list_event_reminders(
    state: State<'_, AppState>,
    event_id: String,
) -> Result<Vec<EventReminder>, VaultError> {
    state
        .inner
        .with_calendar(|conn| events::list_reminders(conn, &event_id))
}

#[tauri::command]
pub fn set_event_reminders(
    state: State<'_, AppState>,
    event_id: String,
    minutes: Vec<i64>,
) -> Result<(), VaultError> {
    state
        .inner
        .with_calendar(|conn| events::set_reminders(conn, &event_id, &minutes))
}

#[tauri::command]
pub fn event_link_state(
    state: State<'_, AppState>,
    event_id: String,
) -> Result<EventLinkState, VaultError> {
    let event = state
        .inner
        .with_calendar(|conn| events::get_event(conn, &event_id))?;
    let page_exists = match event
        .linked_page_ref
        .as_deref()
        .and_then(|value| value.split_once(':'))
    {
        Some((space_id, page_id)) => state
            .inner
            .with_space(space_id, |conn| {
                Ok(conn
                    .query_row(
                        "SELECT 1 FROM pages WHERE id = ?1 AND is_deleted = 0",
                        [page_id],
                        |_| Ok(true),
                    )
                    .optional()?
                    .unwrap_or(false))
            })
            .unwrap_or(false),
        None => false,
    };
    state
        .inner
        .with_tasks(|tasks| events::resolve_links(&event, tasks, |_| Ok(page_exists)))
}

#[tauri::command]
pub fn lunar_overlay_cmd(
    range_start: String,
    range_end: String,
    festivals_enabled: bool,
    solar_terms_enabled: bool,
) -> Result<Vec<DayInfo>, VaultError> {
    let start = date(&range_start)?;
    let end = date(&range_end)?;
    if end < start {
        return Err(VaultError::Validation(
            "范围结束日期不得早于开始日期".into(),
        ));
    }
    Ok(events::lunar_overlay(
        start,
        end,
        festivals_enabled,
        solar_terms_enabled,
    ))
}

#[tauri::command]
pub fn search_events_cmd(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<EventSearchHit>, VaultError> {
    state
        .inner
        .with_calendar(|conn| crate::search::search_events(conn, &query, limit.unwrap_or(50)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_input_maps_clear_flags() {
        let patch: events::EventPatch = EventPatchInput {
            title: Some("改名".into()),
            description: None,
            clear_description: true,
            location: None,
            clear_location: false,
            start_at: None,
            end_at: None,
            all_day: None,
            timezone: None,
            clear_timezone: false,
            recurrence_rule: None,
            clear_recurrence_rule: true,
            lunar_recurrence: None,
            clear_lunar_recurrence: false,
            color: None,
            clear_color: false,
            linked_page_ref: None,
            clear_linked_page_ref: false,
            linked_task_id: None,
            clear_linked_task_id: true,
        }
        .into();
        assert_eq!(patch.title.as_deref(), Some("改名"));
        assert_eq!(patch.description, Some(None));
        assert_eq!(patch.recurrence_rule, Some(None));
        assert_eq!(patch.linked_task_id, Some(None));
        assert!(patch.location.is_none());
    }

    #[test]
    fn date_validation_rejects_invalid_values() {
        assert!(date("2026-02-30").is_err());
        assert_eq!(date("2026-02-17").unwrap().to_string(), "2026-02-17");
    }
}
