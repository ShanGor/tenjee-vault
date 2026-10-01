//! Thin Tauri bridge for the task domain.

use serde::{Deserialize, Serialize};
use tauri::State;

use super::AppState;
use crate::error::{VaultError, VaultResult};
use crate::search::TaskSearchHit;
use crate::tasks::{attachments, lists, tags, tasks};

#[derive(Debug, Deserialize)]
pub struct TaskPatchInput {
    pub title: Option<String>,
    pub notes: Option<String>,
    #[serde(default)]
    pub clear_notes: bool,
    pub priority: Option<String>,
    pub due_date: Option<String>,
    #[serde(default)]
    pub clear_due_date: bool,
    pub due_time: Option<String>,
    #[serde(default)]
    pub clear_due_time: bool,
    pub reminder_at: Option<String>,
    #[serde(default)]
    pub clear_reminder_at: bool,
    pub recurrence_rule: Option<String>,
    #[serde(default)]
    pub clear_recurrence_rule: bool,
    pub list_id: Option<String>,
    pub parent_task_id: Option<String>,
    #[serde(default)]
    pub clear_parent_task_id: bool,
}

impl From<TaskPatchInput> for tasks::TaskPatch {
    fn from(value: TaskPatchInput) -> Self {
        Self {
            title: value.title,
            notes: value.notes.map(Some).or(value.clear_notes.then_some(None)),
            priority: value.priority,
            due_date: value
                .due_date
                .map(Some)
                .or(value.clear_due_date.then_some(None)),
            due_time: value
                .due_time
                .map(Some)
                .or(value.clear_due_time.then_some(None)),
            reminder_at: value
                .reminder_at
                .map(Some)
                .or(value.clear_reminder_at.then_some(None)),
            recurrence_rule: value
                .recurrence_rule
                .map(Some)
                .or(value.clear_recurrence_rule.then_some(None)),
            list_id: value.list_id,
            parent_task_id: value
                .parent_task_id
                .map(Some)
                .or(value.clear_parent_task_id.then_some(None)),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct OpenTaskAttachment {
    pub attachment: attachments::TaskAttachment,
    pub data_base64: String,
}

#[derive(Debug, Deserialize)]
pub struct MedicationDoseInput {
    pub label: String,
    pub time: String,
}

#[tauri::command]
pub fn list_task_lists(state: State<'_, AppState>) -> Result<Vec<lists::TaskList>, VaultError> {
    state.inner.with_tasks(|conn| lists::list_lists(conn))
}

#[tauri::command]
pub fn create_task_list(
    state: State<'_, AppState>,
    name: String,
    color: Option<String>,
) -> Result<lists::TaskList, VaultError> {
    state
        .inner
        .with_tasks(|conn| lists::create_list(conn, &name, color.as_deref()))
}

#[tauri::command]
pub fn rename_task_list(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<(), VaultError> {
    state
        .inner
        .with_tasks(|conn| lists::rename_list(conn, &id, &name))
}

#[tauri::command]
pub fn set_task_list_color(
    state: State<'_, AppState>,
    id: String,
    color: Option<String>,
) -> Result<(), VaultError> {
    state
        .inner
        .with_tasks(|conn| lists::set_list_color(conn, &id, color.as_deref()))
}

#[tauri::command]
pub fn reorder_task_lists(state: State<'_, AppState>, ids: Vec<String>) -> Result<(), VaultError> {
    state
        .inner
        .with_tasks(|conn| lists::reorder_lists(conn, &ids))
}

#[tauri::command]
pub fn delete_task_list(
    state: State<'_, AppState>,
    id: String,
    confirm_non_empty: bool,
) -> Result<(), VaultError> {
    let files_dir = state.inner.tasks_files_dir();
    state
        .inner
        .with_tasks(|conn| lists::delete_list(conn, &files_dir, &id, confirm_non_empty))
}

#[tauri::command]
pub fn create_task_cmd(
    state: State<'_, AppState>,
    list_id: String,
    parent_task_id: Option<String>,
    title: String,
) -> Result<tasks::Task, VaultError> {
    state
        .inner
        .with_tasks(|conn| tasks::create_task(conn, &list_id, parent_task_id.as_deref(), &title))
}

#[tauri::command]
pub fn create_medication_course_tasks(
    state: State<'_, AppState>,
    list_id: String,
    start_date: String,
    days: u32,
    title_prefix: String,
    medicine_name: Option<String>,
    doses: Vec<MedicationDoseInput>,
) -> Result<Vec<tasks::Task>, VaultError> {
    let doses = doses
        .into_iter()
        .map(|dose| (dose.label, dose.time))
        .collect::<Vec<_>>();
    state.inner.with_tasks(|conn| {
        tasks::create_medication_course(
            conn,
            &list_id,
            &start_date,
            days,
            &title_prefix,
            medicine_name.as_deref(),
            &doses,
        )
    })
}

#[tauri::command]
pub fn update_task_cmd(
    state: State<'_, AppState>,
    id: String,
    patch: TaskPatchInput,
) -> Result<tasks::Task, VaultError> {
    state
        .inner
        .with_tasks(|conn| tasks::update_task(conn, &id, patch.into()))
}

#[tauri::command]
pub fn set_task_status_cmd(
    state: State<'_, AppState>,
    id: String,
    status: String,
) -> Result<(), VaultError> {
    state
        .inner
        .with_tasks(|conn| tasks::set_task_status(conn, &id, &status))?;
    super::note_tasks::sync(&state.inner)
}

#[tauri::command]
pub fn delete_task_cmd(state: State<'_, AppState>, id: String) -> Result<(), VaultError> {
    let files_dir = state.inner.tasks_files_dir();
    state
        .inner
        .with_tasks(|conn| tasks::delete_task(conn, &files_dir, &id))
}

#[tauri::command]
pub fn reorder_task_cmd(
    state: State<'_, AppState>,
    id: String,
    new_parent: Option<String>,
    new_sort_order: i64,
) -> Result<(), VaultError> {
    state
        .inner
        .with_tasks(|conn| tasks::reorder_task(conn, &id, new_parent.as_deref(), new_sort_order))
}

#[tauri::command]
pub fn task_list_view(
    state: State<'_, AppState>,
    list_id: String,
) -> Result<Vec<tasks::TaskNode>, VaultError> {
    state
        .inner
        .with_tasks(|conn| tasks::list_view(conn, &list_id))
}

#[tauri::command]
pub fn task_kanban_view(state: State<'_, AppState>) -> Result<Vec<tasks::Task>, VaultError> {
    state.inner.with_tasks(|conn| tasks::kanban_view(conn))
}

#[tauri::command]
pub fn task_smart_view(
    state: State<'_, AppState>,
    view: String,
) -> Result<Vec<tasks::Task>, VaultError> {
    let view = match view.as_str() {
        "today" => tasks::SmartView::Today,
        "week" => tasks::SmartView::Week,
        "overdue" => tasks::SmartView::Overdue,
        _ => return Err(VaultError::Validation(format!("未知智能视图 {view}"))),
    };
    state.inner.with_tasks(|conn| tasks::smart_view(conn, view))
}

#[tauri::command]
pub fn batch_tasks(
    state: State<'_, AppState>,
    ids: Vec<String>,
    action: String,
    target_list_id: Option<String>,
) -> Result<(), VaultError> {
    let action = match action.as_str() {
        "complete" => tasks::BatchAction::Complete,
        "cancel" => tasks::BatchAction::Cancel,
        "delete" => tasks::BatchAction::Delete,
        "move" => tasks::BatchAction::MoveToList(
            target_list_id.ok_or_else(|| VaultError::Validation("缺少目标列表".into()))?,
        ),
        _ => return Err(VaultError::Validation(format!("未知批量操作 {action}"))),
    };
    let files_dir = state.inner.tasks_files_dir();
    state
        .inner
        .with_tasks(|conn| tasks::batch(conn, &files_dir, &ids, action))?;
    super::note_tasks::sync(&state.inner)
}

#[tauri::command]
pub fn archive_tasks_cmd(state: State<'_, AppState>, ids: Vec<String>) -> Result<(), VaultError> {
    state
        .inner
        .with_tasks(|conn| tasks::archive_tasks(conn, &ids))
}

#[tauri::command]
pub fn list_archived_tasks(state: State<'_, AppState>) -> Result<Vec<tasks::Task>, VaultError> {
    state.inner.with_tasks(|conn| tasks::list_archived(conn))
}

#[tauri::command]
pub fn restore_task(state: State<'_, AppState>, id: String) -> Result<(), VaultError> {
    state
        .inner
        .with_tasks(|conn| tasks::unarchive_task(conn, &id))
}

#[tauri::command]
pub fn purge_task(state: State<'_, AppState>, id: String) -> Result<(), VaultError> {
    let files_dir = state.inner.tasks_files_dir();
    state
        .inner
        .with_tasks(|conn| tasks::purge_task(conn, &files_dir, &id))
}

#[tauri::command]
pub fn set_task_tags(
    state: State<'_, AppState>,
    task_id: String,
    tag_ids: Vec<String>,
) -> Result<(), VaultError> {
    state
        .inner
        .with_tasks(|conn| tags::set_task_tags(conn, &task_id, &tag_ids))
}

#[tauri::command]
pub fn get_task_tags(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<Vec<String>, VaultError> {
    state
        .inner
        .with_tasks(|conn| tags::get_task_tags(conn, &task_id))
}

#[tauri::command]
pub fn tasks_by_tag(
    state: State<'_, AppState>,
    tag_id: String,
) -> Result<Vec<tags::TaggedTask>, VaultError> {
    state
        .inner
        .with_tasks(|conn| tags::tasks_by_tag(conn, &tag_id))
}

#[tauri::command]
pub fn save_task_attachment(
    state: State<'_, AppState>,
    task_id: String,
    file_name: String,
    mime: Option<String>,
    data_base64: String,
) -> Result<attachments::TaskAttachment, VaultError> {
    let bytes = crate::notes::base64_decode(&data_base64)?;
    let files_dir = state.inner.tasks_files_dir();
    state.inner.with_tasks(|conn| {
        attachments::save_attachment(
            &files_dir,
            conn,
            &task_id,
            &file_name,
            mime.as_deref(),
            &bytes,
        )
    })
}

#[tauri::command]
pub fn open_task_attachment(
    state: State<'_, AppState>,
    id: String,
) -> Result<OpenTaskAttachment, VaultError> {
    let files_dir = state.inner.tasks_files_dir();
    state.inner.with_tasks(|conn| {
        let (attachment, data_base64) = attachments::open_attachment(&files_dir, conn, &id)?;
        Ok(OpenTaskAttachment {
            attachment,
            data_base64,
        })
    })
}

#[tauri::command]
pub fn delete_task_attachment(state: State<'_, AppState>, id: String) -> Result<(), VaultError> {
    let files_dir = state.inner.tasks_files_dir();
    state
        .inner
        .with_tasks(|conn| attachments::delete_attachment(&files_dir, conn, &id))
}

#[tauri::command]
pub fn list_task_attachments(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<Vec<attachments::TaskAttachment>, VaultError> {
    state
        .inner
        .with_tasks(|conn| attachments::list_attachments(conn, &task_id))
}

#[tauri::command]
pub fn search_tasks_cmd(
    state: State<'_, AppState>,
    query: String,
    include_archived: bool,
    limit: Option<usize>,
) -> Result<Vec<TaskSearchHit>, VaultError> {
    state.inner.with_tasks(|conn| {
        crate::search::search_tasks(conn, &query, include_archived, limit.unwrap_or(50))
    })
}

pub fn create_task_for_test(
    inner: &super::AppStateInner,
    list_id: &str,
    title: &str,
) -> VaultResult<tasks::Task> {
    inner.with_tasks(|conn| tasks::create_task(conn, list_id, None, title))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_input_maps_clear_flags() {
        let patch: tasks::TaskPatch = TaskPatchInput {
            title: Some("任务".into()),
            notes: None,
            clear_notes: true,
            priority: Some("high".into()),
            due_date: None,
            clear_due_date: true,
            due_time: None,
            clear_due_time: false,
            reminder_at: None,
            clear_reminder_at: false,
            recurrence_rule: None,
            clear_recurrence_rule: true,
            list_id: None,
            parent_task_id: None,
            clear_parent_task_id: true,
        }
        .into();
        assert_eq!(patch.notes, Some(None));
        assert_eq!(patch.due_date, Some(None));
        assert_eq!(patch.recurrence_rule, Some(None));
        assert_eq!(patch.parent_task_id, Some(None));
        assert!(patch.due_time.is_none());
    }
}
