//! Tenjee Vault — 纯本地离线个人生产力工具（日程 / 任务 / 笔记，含加密分区）。
//! M1：项目骨架、多库架构与迁移体系、加密原语。
//! M2：笔记模块（层级组织、TipTap 编辑器、加密分区、FTS5 搜索）。

pub mod backup;
pub mod blob_store;
pub mod calendar;
pub mod commands;
pub mod crypto;
pub mod db;
pub mod desktop;
pub mod error;
pub mod notes;
pub mod portability;
pub mod remind;
pub mod search;
pub mod tags;
pub mod tasks;
#[cfg(test)]
mod release_tests;

use commands::AppState;
use tauri::{Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(tauri_plugin_window_state::StateFlags::POSITION)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
            let _ = app.emit("app-action", "open-command-palette");
        }))
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    let close_to_tray = commands::settings_of(&state.inner)
                        .map(|settings| settings.close_to_tray)
                        .unwrap_or(false);
                    let tray_available = window
                        .app_handle()
                        .try_state::<desktop::DesktopCapabilities>()
                        .map(|capabilities| capabilities.tray_available())
                        .unwrap_or(false);
                    if close_to_tray && tray_available {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
            }
        })
        .setup(|app| {
            if !commands::release_probe_enabled() { let _ = app.notification().request_permission(); }
            let app_data = std::env::var_os("TENJEE_RELEASE_PROBE_DIR").map(std::path::PathBuf::from)
                .unwrap_or_else(|| app.path().app_data_dir().expect("无法确定应用数据目录"));
            let root = db::layout::data_root(&app_data);
            // This happens before layout initialization or any long-lived SQLite connection is
            // opened, which keeps restore exchange atomic and makes rollback meaningful.
            let restore_alert = db::restore::apply_pending_restore(&root)
                .err()
                .map(|error| error.to_string());
            // 启动流程：布局初始化 + 迁移 + 完整性检查；失败降级为仅含告警的报告，
            // 应用仍可启动（单库隔离原则同样适用于启动本身）。
            let report = db::startup::startup(&root).unwrap_or_else(|e| {
                let mut report = db::startup::StartupReport::default();
                report.alerts.push(format!("启动检查失败: {e}"));
                report
            });
            let mut report = report;
            if let Some(alert) = restore_alert {
                report.alerts.push(alert);
            }
            let state = AppState::init(root, report).expect("应用状态初始化失败");
            let reminder_inner = state.inner.clone();
            let reminder_app = app.handle().clone();
            let reminder_service = remind::ReminderService::new(
                remind::SystemClock,
                remind::TauriNotifier(reminder_app.clone()),
            );
            let emit_notices = move |notices: &[remind::InAppNotice]| {
                for notice in notices {
                    let _ = reminder_app.emit("reminder-in-app", notice);
                }
            };
            if let Ok(report) = reminder_inner.with_core_connections(|meta, tasks, calendar| {
                reminder_service.startup_catch_up(meta, tasks, calendar)
            }) {
                reminder_inner.push_in_app_reminders(report.in_app);
            }
            std::thread::spawn(move || {
                let mut since = chrono::Local::now().naive_local();
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(30));
                    let report = reminder_inner.with_core_connections(|meta, tasks, calendar| {
                        reminder_service.poll(meta, tasks, calendar, since)
                    });
                    since = chrono::Local::now().naive_local();
                    if let Ok(report) = report {
                        emit_notices(&report.in_app);
                    }
                }
            });
            // 后台巡检线程：闲置超时锁定的最终裁决（前端心跳之外的双保险，design D2）。
            // 锁定结果由前端心跳/命令响应感知（线程无事件通道）。
            let inner = state.inner.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(60));
                inner.sweep_idle();
            });
            // A startup check supplies one missed period; subsequent checks run at a modest
            // cadence and report failures to the frontend without compromising existing files.
            let scheduler_inner = state.inner.clone();
            let scheduler_app = app.handle().clone();
            std::thread::spawn(move || {
                while !scheduler_inner.backup_scheduler_stopped() {
                    match backup::scheduler::run_if_due(&scheduler_inner, chrono::Utc::now()) {
                        Ok(true) => {
                            let _ =
                                scheduler_app.emit("backup-status", "automatic backup completed");
                        }
                        Ok(false) => {}
                        Err(error) => {
                            let _ = scheduler_app
                                .emit("backup-status", format!("automatic backup failed: {error}"));
                        }
                    }
                    for _ in 0..60 {
                        if scheduler_inner.backup_scheduler_stopped() {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_secs(1));
                    }
                }
            });
            app.manage(state);
            app.manage(desktop::DesktopCapabilities::default());
            desktop::install(&app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::release_probe_enabled,
            commands::release_probe_ready,
            desktop::set_runtime_locale,
            commands::db_status,
            commands::drain_in_app_reminders,
            commands::list_spaces,
            commands::create_space_cmd,
            commands::rename_space_cmd,
            commands::archive_space_cmd,
            commands::delete_space_cmd,
            commands::ensure_default_space,
            commands::get_settings,
            commands::set_setting_cmd,
            commands::reset_settings_group,
            commands::global_search::global_search_cmd,
            commands::tags::list_tags,
            commands::templates::list_templates,
            commands::note_tasks::link_note_task,
            commands::note_tasks::inspect_note_task,
            commands::note_tasks::unlink_note_task,
            commands::templates::save_page_template,
            commands::templates::edit_template,
            commands::templates::export_template_global,
            commands::templates::create_page_from_template,
            commands::tags::query_tag,
            commands::tags::create_tag,
            commands::tags::update_tag,
            commands::tags::delete_tag,
            commands::tags::set_page_tags,
            commands::tags::get_page_tags,
            commands::tags::set_event_tags,
            commands::tags::get_event_tags,
            commands::backup::create_backup_cmd,
            commands::backup::verify_backup_cmd,
            commands::backup::prepare_restore_cmd,
            commands::backup::get_restore_diagnostic_cmd,
            commands::backup::clear_pre_restore_copies_cmd,
            commands::tasks::list_task_lists,
            commands::tasks::create_task_list,
            commands::tasks::rename_task_list,
            commands::tasks::set_task_list_color,
            commands::tasks::reorder_task_lists,
            commands::tasks::delete_task_list,
            commands::tasks::create_task_cmd,
            commands::tasks::create_medication_course_tasks,
            commands::tasks::update_task_cmd,
            commands::tasks::set_task_status_cmd,
            commands::tasks::delete_task_cmd,
            commands::tasks::reorder_task_cmd,
            commands::tasks::task_list_view,
            commands::tasks::task_kanban_view,
            commands::tasks::task_smart_view,
            commands::tasks::batch_tasks,
            commands::tasks::archive_tasks_cmd,
            commands::tasks::list_archived_tasks,
            commands::tasks::restore_task,
            commands::tasks::purge_task,
            commands::tasks::set_task_tags,
            commands::tasks::get_task_tags,
            commands::tasks::tasks_by_tag,
            commands::tasks::save_task_attachment,
            commands::tasks::open_task_attachment,
            commands::tasks::delete_task_attachment,
            commands::tasks::list_task_attachments,
            commands::tasks::search_tasks_cmd,
            commands::calendar::create_event_cmd,
            commands::calendar::get_event_cmd,
            commands::calendar::update_event_cmd,
            commands::calendar::delete_event_cmd,
            commands::calendar::calendar_instances,
            commands::calendar::move_event_instance,
            commands::calendar::cancel_event_instance,
            commands::calendar::list_event_reminders,
            commands::calendar::set_event_reminders,
            commands::calendar::event_link_state,
            commands::calendar::lunar_overlay_cmd,
            commands::calendar::search_events_cmd,
            commands::calendar::import_calendar_ical_cmd,
            commands::calendar::export_calendar_ical_cmd,
            commands::notes::get_tree,
            commands::notes::get_page_metadata_cmd,
            commands::notes::create_space_page_cmd,
            commands::crypto_cmds::set_page_password_cmd,
            commands::notes::get_page_tree,
            commands::notes::create_notebook_cmd,
            commands::notes::rename_notebook_cmd,
            commands::notes::set_notebook_color_cmd,
            commands::notes::reorder_notebooks_cmd,
            commands::notes::delete_notebook_cmd,
            commands::notes::create_section_group_cmd,
            commands::notes::rename_section_group_cmd,
            commands::notes::move_section_group_cmd,
            commands::notes::delete_section_group_cmd,
            commands::notes::create_section_cmd,
            commands::notes::rename_section_cmd,
            commands::notes::set_section_color_cmd,
            commands::notes::move_section_cmd,
            commands::notes::reorder_sections_cmd,
            commands::notes::delete_section_cmd,
            commands::notes::create_page_cmd,
            commands::notes::rename_page_cmd,
            commands::notes::move_page_cmd,
            commands::notes::delete_page_cmd,
            commands::notes::restore_page_cmd,
            commands::notes::purge_page_cmd,
            commands::notes::list_trash_cmd,
            commands::notes::get_page_cmd,
            commands::notes::save_page_cmd,
            commands::notes::list_versions_cmd,
            commands::notes::rollback_version_cmd,
            commands::notes::import_note_files_cmd,
            commands::notes::request_page_export_confirmation_cmd,
            commands::notes::request_section_export_confirmation_cmd,
            commands::notes::export_page_cmd,
            commands::notes::request_subtree_export_confirmation_cmd,
            commands::notes::export_subtree_cmd,
            commands::notes::export_section_cmd,
            commands::notes::record_page_open_cmd,
            commands::notes::list_recent_cmd,
            commands::notes::list_page_titles_cmd,
            commands::notes::save_attachment_cmd,
            commands::notes::open_attachment_cmd,
            commands::notes::delete_attachment_cmd,
            commands::notes::list_attachments_cmd,
            commands::notes::search_notes_cmd,
            commands::crypto_cmds::set_section_password_cmd,
            commands::crypto_cmds::unlock_section_cmd,
            commands::crypto_cmds::lock_section_cmd,
            commands::crypto_cmds::lock_all_sections_cmd,
            commands::crypto_cmds::change_section_password_cmd,
            commands::crypto_cmds::remove_section_password_cmd,
            commands::crypto_cmds::activity_heartbeat_cmd,
            commands::crypto_cmds::get_unlocked_sections_cmd,
            commands::crypto_cmds::generate_password_cmd,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // 应用退出：全量锁定（密钥清零 + 内存索引随进程退出销毁，design D2/D4）
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<AppState>() {
                    state.inner.session.lock_all();
                    state.inner.stop_backup_scheduler();
                }
            }
        });
}
