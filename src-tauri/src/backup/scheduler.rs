//! In-process automatic backup scheduling.  It intentionally records one successful period,
//! not a queue of missed jobs, so a long-offline machine cannot create a backup storm.

use std::path::Path;

use chrono::{DateTime, Datelike, Utc};

use crate::backup::{reader, writer};
use crate::commands::{self, settings_of, AppStateInner, SETTING_AUTO_BACKUP_LAST_SUCCESS};
use crate::error::{VaultError, VaultResult};

fn period_key(now: DateTime<Utc>, schedule: &str) -> VaultResult<String> {
    match schedule {
        "daily" => Ok(format!("{}-{:03}", now.year(), now.ordinal())),
        "weekly" => {
            let week = now.iso_week();
            Ok(format!("{}-{:02}", week.year(), week.week()))
        }
        _ => Err(VaultError::Validation(
            "自动备份周期必须是 daily 或 weekly".into(),
        )),
    }
}

pub fn is_due(now: DateTime<Utc>, last_success: &str, schedule: &str) -> VaultResult<bool> {
    if last_success.is_empty() {
        return Ok(true);
    }
    let last = DateTime::parse_from_rfc3339(last_success)
        .map_err(|_| VaultError::Validation("自动备份上次成功时间无效".into()))?
        .with_timezone(&Utc);
    Ok(period_key(now, schedule)? != period_key(last, schedule)?)
}

fn purge_excess(directory: &Path, retention: usize) -> VaultResult<()> {
    let mut verified = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_file()
            && path.extension().and_then(|value| value.to_str()) == Some("tvault")
        {
            if let Ok(manifest) = reader::verify_archive(&path) {
                if manifest.managed_auto_backup {
                    verified.push((manifest.created_at, path));
                }
            }
        }
    }
    verified.sort_by(|left, right| left.0.cmp(&right.0));
    let excess = verified.len().saturating_sub(retention);
    for (_, path) in verified.into_iter().take(excess) {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Create at most one due package.  The writer's maintenance gate makes this mutually
/// exclusive with manual backup/restore snapshots.
pub fn run_if_due(inner: &AppStateInner, now: DateTime<Utc>) -> VaultResult<bool> {
    let settings = settings_of(inner)?;
    if !settings.auto_backup_enabled {
        return Ok(false);
    }
    if settings.auto_backup_directory.is_empty() {
        return Err(VaultError::Validation("已启用自动备份但未选择目录".into()));
    }
    if !is_due(
        now,
        &settings.auto_backup_last_success_at,
        &settings.auto_backup_schedule,
    )? {
        return Ok(false);
    }
    let directory = Path::new(&settings.auto_backup_directory);
    if !directory.is_dir() {
        return Err(VaultError::Validation("自动备份目录不可用".into()));
    }
    let filename = format!(
        "tenjee-vault-auto-{}-{}.tvault",
        now.format("%Y%m%dT%H%M%SZ"),
        uuid::Uuid::new_v4()
    );
    writer::create_backup(inner, &directory.join(filename), false, true)?;
    commands::set_setting(inner, SETTING_AUTO_BACKUP_LAST_SUCCESS, &now.to_rfc3339())?;
    purge_excess(directory, settings.auto_backup_retention_count as usize)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn only_one_missed_period_is_due() {
        let now = Utc.with_ymd_and_hms(2026, 9, 21, 12, 0, 0).unwrap();
        assert!(is_due(now, "", "daily").unwrap());
        assert!(!is_due(now, "2026-09-21T00:01:00Z", "daily").unwrap());
        assert!(is_due(now, "2026-09-20T23:59:00Z", "daily").unwrap());
        assert!(!is_due(now, "2026-09-21T00:00:00Z", "weekly").unwrap());
    }
}
