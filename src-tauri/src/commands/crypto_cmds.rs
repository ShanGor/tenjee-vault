//! 加密分区 commands：设置/修改/移除密码、解锁、锁定、活动心跳（design D2/D3）。

use tauri::{AppHandle, Emitter, State};

use super::{AppState, AppStateInner};
use crate::error::{VaultError, VaultResult};
use crate::notes::sections_crypto;

fn emit_locked(app: &AppHandle, section_id: &str) {
    let _ = app.emit(
        "section-locked",
        serde_json::json!({ "section_id": section_id }),
    );
}

/// 解锁后重建该分区的内存临时搜索索引（design D4）。
fn build_index(inner: &AppStateInner, space_id: &str, section_id: &str) -> VaultResult<()> {
    let session = &inner.session;
    inner.with_space(space_id, |conn| {
        let decrypted = sections_crypto::decrypted_pages(conn, session, section_id)?;
        crate::search::build_unlocked_index(conn, section_id, &decrypted)
    })
}

/// 设置分区密码：确认不可恢复 → 生成 DSK 并批量加密既有页面 → 建立会话并建内存索引。
#[tauri::command]
pub fn set_section_password_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
    space_id: String,
    section_id: String,
    password: String,
    confirm_irrecoverable: bool,
) -> Result<(), VaultError> {
    if password.is_empty() {
        return Err(VaultError::Validation("密码不能为空".into()));
    }
    let keys = state.inner.with_space(&space_id, |conn| {
        sections_crypto::set_password(conn, &section_id, &password, confirm_irrecoverable)
    })?;
    state.inner.session.insert(&section_id, keys);
    build_index(&state.inner, &space_id, &section_id)?;
    let _ = app;
    Ok(())
}

#[tauri::command]
pub fn unlock_section_cmd(
    state: State<'_, AppState>,
    space_id: String,
    section_id: String,
    password: String,
) -> Result<(), VaultError> {
    let keys = state.inner.with_space(&space_id, |conn| {
        sections_crypto::unlock_keys(conn, &section_id, &password)
    })?;
    state.inner.session.insert(&section_id, keys);
    super::note_tasks::sync(&state.inner)?;
    build_index(&state.inner, &space_id, &section_id)
}

#[tauri::command]
pub fn lock_section_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
    space_id: String,
    section_id: String,
) -> Result<(), VaultError> {
    state.inner.session.lock(&section_id);
    state.inner.with_space(&space_id, |conn| {
        crate::search::drop_unlocked_index(conn, &section_id)
    })?;
    emit_locked(&app, &section_id);
    Ok(())
}

#[tauri::command]
pub fn lock_all_sections_cmd(app: AppHandle, state: State<'_, AppState>) -> Result<(), VaultError> {
    let locked = state.inner.session.unlocked_section_ids();
    state.inner.session.lock_all();
    // 临时表随连接：每个常驻连接上都执行 DROP（幂等）
    if let Ok(guard) = state.inner.spaces() {
        for section_id in &locked {
            for conn in guard.values() {
                let _ = crate::search::drop_unlocked_index(conn, section_id);
            }
        }
    }
    for section_id in &locked {
        emit_locked(&app, section_id);
    }
    Ok(())
}

/// 修改密码：验证旧密码后仅重包裹 DSK（会话内 DSK 不变，保持解锁态）。
#[tauri::command]
pub fn change_section_password_cmd(
    state: State<'_, AppState>,
    space_id: String,
    section_id: String,
    old_password: String,
    new_password: String,
) -> Result<(), VaultError> {
    if new_password.is_empty() {
        return Err(VaultError::Validation("新密码不能为空".into()));
    }
    state.inner.with_space(&space_id, |conn| {
        sections_crypto::change_password(conn, &section_id, &old_password, &new_password)
    })
}

/// 移除密码：验证后批量解密回明文。
#[tauri::command]
pub fn remove_section_password_cmd(
    state: State<'_, AppState>,
    space_id: String,
    section_id: String,
    password: String,
) -> Result<(), VaultError> {
    state.inner.with_space(&space_id, |conn| {
        let root: Option<String> = conn.query_row(
            "SELECT root_page_id FROM sections WHERE id=?1",
            [&section_id],
            |r| r.get(0),
        )?;
        if root.is_some() {
            crate::notes::page_tree::remove_protection(
                conn,
                &state.inner.files_dir(&space_id),
                &state.inner.session,
                &section_id,
                &password,
            )
        } else {
            sections_crypto::remove_password(conn, &state.inner.session, &section_id, &password)
        }
    })
}

/// 活动心跳：刷新全部解锁分区的活动时间，巡检闲置并锁定 + 销毁内存索引，
/// 返回本次锁定的分区 id（前端据此同步导航/编辑器/搜索视图）。
#[tauri::command]
pub fn activity_heartbeat_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<String>, VaultError> {
    state.inner.session.touch_all();
    let locked = state.inner.sweep_idle();
    for section_id in &locked {
        emit_locked(&app, section_id);
    }
    Ok(locked)
}

#[tauri::command]
pub fn get_unlocked_sections_cmd(state: State<'_, AppState>) -> Result<Vec<String>, VaultError> {
    Ok(state.inner.session.unlocked_section_ids())
}

/// 内置密码生成器入口（spec: 加密分区设置与确认，沿用 crypto-core 生成能力）。
#[tauri::command]
pub fn generate_password_cmd(length: Option<usize>) -> Result<String, VaultError> {
    Ok(crate::crypto::password_gen::generate_password(
        length.unwrap_or(crate::crypto::password_gen::DEFAULT_LENGTH),
    ))
}

#[tauri::command]
pub fn set_page_password_cmd(
    state: State<'_, AppState>,
    space_id: String,
    page_id: String,
    password: String,
    confirm_irrecoverable: bool,
) -> VaultResult<()> {
    state.inner.with_space(&space_id, |conn| {
        crate::notes::page_tree::protect(
            conn,
            &state.inner.files_dir(&space_id),
            &state.inner.session,
            &page_id,
            &password,
            confirm_irrecoverable,
        )?;
        super::notes::rebuild_indexes(&state.inner, conn)
    })
}
