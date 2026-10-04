//! Reminder polling and startup catch-up (design D6).

use chrono::{Duration, Local, NaiveDate, NaiveDateTime};
use rusqlite::{params, Connection};
use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_notification::{NotificationExt, PermissionState};

use crate::calendar::events::{instances_in_range, EventInstance};
use crate::error::{VaultError, VaultResult};

const TIMESTAMP_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";
const CATCH_UP_SUMMARY_LIMIT: usize = 20;

pub trait Clock: Send + Sync {
    fn now(&self) -> NaiveDateTime;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> NaiveDateTime {
        Local::now().naive_local()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyError {
    PermissionDenied,
    Transport,
}

pub trait Notifier: Send + Sync {
    fn notify(&self, title: &str, body: &str) -> Result<(), NotifyError>;
}

#[derive(Clone)]
pub struct TauriNotifier(pub AppHandle);

impl Notifier for TauriNotifier {
    fn notify(&self, title: &str, body: &str) -> Result<(), NotifyError> {
        let notification = self.0.notification();
        match notification
            .permission_state()
            .map_err(|_| NotifyError::Transport)?
        {
            PermissionState::Granted => notification
                .builder()
                .title(title)
                .body(body)
                .show()
                .map_err(|_| NotifyError::Transport),
            PermissionState::Denied
            | PermissionState::Prompt
            | PermissionState::PromptWithRationale => Err(NotifyError::PermissionDenied),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InAppNotice {
    pub title: String,
    pub body: String,
    pub kind: String,
    pub count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DispatchReport {
    pub delivered: usize,
    pub in_app: Vec<InAppNotice>,
}

#[derive(Debug, Clone)]
struct Candidate {
    entity_kind: &'static str,
    entity_id: String,
    occurrence_key: String,
    slot: i64,
    title: String,
    body: String,
    fire_at: NaiveDateTime,
    relevant_at_startup: bool,
}

pub struct ReminderService<C, N> {
    clock: C,
    notifier: N,
}

impl<C: Clock, N: Notifier> ReminderService<C, N> {
    pub fn new(clock: C, notifier: N) -> Self {
        Self { clock, notifier }
    }

    /// Poll reminders that became due after `since`. Persistent fire records make
    /// overlapping polling windows safe.
    pub fn poll(
        &self,
        meta: &Connection,
        tasks: &Connection,
        calendar: &Connection,
        since: NaiveDateTime,
    ) -> VaultResult<DispatchReport> {
        let now = self.clock.now();
        let candidates = collect_candidates(tasks, calendar, since.date(), now)?
            .into_iter()
            .filter(|candidate| candidate.fire_at > since && candidate.fire_at <= now)
            .collect();
        self.dispatch(meta, candidates, false)
    }

    /// Deliver reminders missed while the app was stopped, but only while the
    /// corresponding task/event occurrence remains relevant.
    pub fn startup_catch_up(
        &self,
        meta: &Connection,
        tasks: &Connection,
        calendar: &Connection,
    ) -> VaultResult<DispatchReport> {
        let now = self.clock.now();
        let candidates = collect_candidates(tasks, calendar, now.date(), now)?
            .into_iter()
            .filter(|candidate| candidate.fire_at <= now && candidate.relevant_at_startup)
            .collect();
        self.dispatch(meta, candidates, true)
    }

    fn dispatch(
        &self,
        meta: &Connection,
        candidates: Vec<Candidate>,
        summarize: bool,
    ) -> VaultResult<DispatchReport> {
        let mut pending = Vec::new();
        let locale: String = meta.query_row("SELECT value FROM app_config WHERE key='runtime_locale'",[],|row|row.get(0)).unwrap_or_else(|_| "en".into());
        let zh = locale == "zh-CN";
        for candidate in candidates {
            let fired: bool = meta.query_row(
                "SELECT EXISTS(SELECT 1 FROM reminder_fires WHERE entity_kind = ?1 AND entity_id = ?2 AND occurrence_key = ?3 AND slot = ?4)",
                params![candidate.entity_kind, candidate.entity_id, candidate.occurrence_key, candidate.slot],
                |row| row.get(0),
            )?;
            if !fired {
                pending.push(candidate);
            }
        }

        if summarize && pending.len() > CATCH_UP_SUMMARY_LIMIT {
            let notice = InAppNotice {
                title: if zh {"Tenjee Vault 提醒"} else {"Tenjee Vault reminder"}.into(),
                body: if zh {format!("你有 {} 条停机期间错过的提醒", pending.len())} else {format!("You have {} missed reminders",pending.len())},
                kind: "summary".into(), count: pending.len(),
            };
            let mut report = DispatchReport::default();
            match self.notifier.notify(&notice.title, &notice.body) {
                Ok(()) => report.delivered = 1,
                Err(NotifyError::PermissionDenied) => report.in_app.push(notice),
                Err(NotifyError::Transport) => {
                    return Err(VaultError::Validation("系统通知发送失败".into()));
                }
            }
            for candidate in &pending {
                record_fire(meta, candidate)?;
            }
            return Ok(report);
        }

        let mut report = DispatchReport::default();
        for mut candidate in pending {
            candidate.title = match (candidate.entity_kind,zh) { ("task",true)=>"任务提醒",("task",false)=>"Task reminder",(_,true)=>"日程提醒",(_,false)=>"Calendar reminder" }.into();
            match self.notifier.notify(&candidate.title, &candidate.body) {
                Ok(()) => report.delivered += 1,
                Err(NotifyError::PermissionDenied) => report.in_app.push(InAppNotice {
                    title: candidate.title.clone(),
                    body: candidate.body.clone(),
                    kind: candidate.entity_kind.into(), count: 1,
                }),
                Err(NotifyError::Transport) => {
                    return Err(VaultError::Validation("系统通知发送失败".into()));
                }
            }
            record_fire(meta, &candidate)?;
        }
        Ok(report)
    }
}

fn record_fire(meta: &Connection, candidate: &Candidate) -> VaultResult<()> {
    meta.execute(
        "INSERT OR IGNORE INTO reminder_fires (entity_kind, entity_id, occurrence_key, slot) VALUES (?1, ?2, ?3, ?4)",
        params![candidate.entity_kind, candidate.entity_id, candidate.occurrence_key, candidate.slot],
    )?;
    Ok(())
}

/// Android owns delivery for this bounded plan; desktop polling remains separate.
pub fn scheduled_plan(meta: &Connection, tasks: &Connection, calendar: &Connection, horizon: NaiveDateTime)
    -> VaultResult<(Vec<crate::mobile::reminders::ScheduledReminder>, usize)> {
    let now = Local::now().naive_local();
    let zh = meta.query_row("SELECT value FROM app_config WHERE key='runtime_locale'", [], |r| r.get::<_,String>(0))
        .unwrap_or_else(|_| "en".into()) == "zh-CN";
    let max_minutes: i64 = calendar.query_row("SELECT COALESCE(MAX(minutes_before),0) FROM event_reminders", [], |r| r.get(0))?;
    let mut candidates = collect_candidates_through(tasks, calendar, now.date(), now, (horizon + Duration::minutes(max_minutes) + Duration::days(1)).date())?;
    candidates.retain(|c| c.fire_at <= horizon && (c.fire_at > now || c.relevant_at_startup));
    candidates.sort_by_key(|c| c.fire_at);
    let mut plan = Vec::new();
    let mut deferred = 0;
    for mut c in candidates {
        let legacy_fired: bool = meta.query_row("SELECT EXISTS(SELECT 1 FROM reminder_fires WHERE entity_kind=?1 AND entity_id=?2 AND occurrence_key=?3 AND slot=?4)",
            params![c.entity_kind,c.entity_id,c.occurrence_key,c.slot], |r| r.get(0))?;
        if legacy_fired && c.fire_at <= now { continue; }
        c.occurrence_key = format!("{}|{}",c.occurrence_key,c.fire_at.format(TIMESTAMP_FORMAT));
        let fired: bool = meta.query_row("SELECT EXISTS(SELECT 1 FROM reminder_fires WHERE entity_kind=?1 AND entity_id=?2 AND occurrence_key=?3 AND slot=?4)",
            params![c.entity_kind,c.entity_id,c.occurrence_key,c.slot], |r| r.get(0))?;
        if fired { continue; }
        if plan.len() == 256 { deferred += 1; continue; }
        let identity = serde_json::to_vec(&(c.entity_kind, &c.entity_id, &c.occurrence_key, c.slot))
            .map_err(|_| VaultError::Validation("Cannot identify reminder".into()))?;
        plan.push(crate::mobile::reminders::ScheduledReminder {
            key: crate::blob_store::hash_hex(&identity), entity_kind:c.entity_kind.into(), entity_id:c.entity_id,
            occurrence_key:c.occurrence_key, slot:c.slot,
            title: match (c.entity_kind,zh) { ("task",true)=>"任务提醒",("task",false)=>"Task reminder",(_,true)=>"日程提醒",(_,false)=>"Calendar reminder" }.into(),
            body:c.body, at:c.fire_at.format(TIMESTAMP_FORMAT).to_string(), when_ms:crate::mobile::reminders::timestamp(c.fire_at)?,
        });
    }
    Ok((plan,deferred))
}

fn parse_timestamp(value: &str) -> VaultResult<NaiveDateTime> {
    NaiveDateTime::parse_from_str(value, TIMESTAMP_FORMAT)
        .map_err(|_| VaultError::Validation(format!("提醒时间 {value} 非法")))
}

fn instance_end(instance: &EventInstance) -> VaultResult<NaiveDateTime> {
    if instance.event.all_day {
        Ok(NaiveDate::parse_from_str(&instance.end_at, "%Y-%m-%d")
            .map_err(|_| VaultError::Validation(format!("事件结束日期 {} 非法", instance.end_at)))?
            .succ_opt()
            .ok_or_else(|| VaultError::Validation("事件结束日期超出范围".into()))?
            .and_hms_opt(0, 0, 0)
            .expect("midnight exists"))
    } else {
        parse_timestamp(&instance.end_at)
    }
}

fn instance_start(instance: &EventInstance) -> VaultResult<NaiveDateTime> {
    if instance.event.all_day {
        Ok(NaiveDate::parse_from_str(&instance.start_at, "%Y-%m-%d")
            .map_err(|_| {
                VaultError::Validation(format!("事件开始日期 {} 非法", instance.start_at))
            })?
            .and_hms_opt(0, 0, 0)
            .expect("midnight exists"))
    } else {
        parse_timestamp(&instance.start_at)
    }
}

fn collect_candidates(
    tasks: &Connection,
    calendar: &Connection,
    range_start: NaiveDate,
    now: NaiveDateTime,
) -> VaultResult<Vec<Candidate>> {
    let max_minutes: i64 = calendar.query_row("SELECT COALESCE(MAX(minutes_before),0) FROM event_reminders", [], |r| r.get(0))?;
    collect_candidates_through(tasks, calendar, range_start, now, (now + Duration::minutes(max_minutes) + Duration::days(1)).date())
}

fn collect_candidates_through(tasks: &Connection, calendar: &Connection, range_start: NaiveDate, now: NaiveDateTime, range_end: NaiveDate) -> VaultResult<Vec<Candidate>> {
    let mut candidates = Vec::new();
    let mut task_stmt = tasks.prepare(
        "SELECT id, title, reminder_at FROM tasks
         WHERE reminder_at IS NOT NULL AND archived_at IS NULL AND status IN ('todo', 'in_progress')",
    )?;
    let rows = task_stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (id, title, reminder_at) = row?;
        candidates.push(Candidate {
            entity_kind: "task",
            entity_id: id,
            occurrence_key: String::new(),
            slot: 0,
            title: "任务提醒".into(),
            body: title,
            fire_at: parse_timestamp(&reminder_at)?,
            relevant_at_startup: true,
        });
    }

    for instance in instances_in_range(calendar, range_start, range_end)? {
        let start = instance_start(&instance)?;
        let end = instance_end(&instance)?;
        let mut reminder_stmt = calendar.prepare(
            "SELECT minutes_before FROM event_reminders WHERE event_id = ?1 ORDER BY minutes_before",
        )?;
        let reminders = reminder_stmt
            .query_map(params![instance.event.id], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for minutes_before in reminders {
            candidates.push(Candidate {
                entity_kind: "event",
                entity_id: instance.event.id.clone(),
                occurrence_key: instance.original_start_at.clone(),
                slot: minutes_before,
                title: "日程提醒".into(),
                body: instance.event.title.clone(),
                fire_at: start - Duration::minutes(minutes_before),
                relevant_at_startup: end > now,
            });
        }
    }
    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::calendar::events::{create_event, set_reminders};

    struct FakeClock(NaiveDateTime);
    impl Clock for FakeClock {
        fn now(&self) -> NaiveDateTime {
            self.0
        }
    }

    #[derive(Default)]
    struct FakeNotifier {
        sent: Mutex<Vec<(String, String)>>,
        denied: bool,
    }
    impl Notifier for FakeNotifier {
        fn notify(&self, title: &str, body: &str) -> Result<(), NotifyError> {
            if self.denied {
                return Err(NotifyError::PermissionDenied);
            }
            self.sent
                .lock()
                .unwrap()
                .push((title.to_string(), body.to_string()));
            Ok(())
        }
    }

    fn databases() -> (Connection, Connection, Connection) {
        fn db(kind: crate::db::migrate::DbKind) -> Connection {
            let mut conn = Connection::open_in_memory().unwrap();
            crate::db::connection::configure(&conn).unwrap();
            crate::db::migrate::run_migrations(&mut conn, kind.migrations()).unwrap();
            conn
        }
        (
            db(crate::db::migrate::DbKind::Meta),
            db(crate::db::migrate::DbKind::Tasks),
            db(crate::db::migrate::DbKind::Calendar),
        )
    }

    fn dt(value: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(value, TIMESTAMP_FORMAT).unwrap()
    }

    #[test]
    fn due_task_fires_once() {
        let (meta, tasks, calendar) = databases();
        tasks
            .execute(
                "INSERT INTO task_lists (id, name) VALUES ('l', '收件箱')",
                [],
            )
            .unwrap();
        tasks.execute(
            "INSERT INTO tasks (id, list_id, title, reminder_at) VALUES ('t', 'l', '交周报', '2026-09-20T10:00:00')",
            [],
        ).unwrap();
        let service = ReminderService::new(
            FakeClock(dt("2026-09-20T10:00:10")),
            FakeNotifier::default(),
        );
        let first = service
            .poll(&meta, &tasks, &calendar, dt("2026-09-20T09:59:40"))
            .unwrap();
        assert_eq!(first.delivered, 1);
        let second = service
            .poll(&meta, &tasks, &calendar, dt("2026-09-20T09:59:40"))
            .unwrap();
        assert_eq!(second.delivered, 0, "fire record prevents duplicates");
    }

    #[test]
    fn recurring_occurrences_fire_independently() {
        let (meta, tasks, calendar) = databases();
        let event = create_event(
            &calendar,
            "周会",
            None,
            None,
            "2026-09-20T10:00:00",
            "2026-09-20T11:00:00",
            false,
            None,
            Some("FREQ=DAILY;COUNT=2"),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        set_reminders(&calendar, &event.id, &[10]).unwrap();
        let first = ReminderService::new(
            FakeClock(dt("2026-09-20T09:50:10")),
            FakeNotifier::default(),
        );
        assert_eq!(
            first
                .poll(&meta, &tasks, &calendar, dt("2026-09-20T09:49:40"))
                .unwrap()
                .delivered,
            1
        );
        let second = ReminderService::new(
            FakeClock(dt("2026-09-21T09:50:10")),
            FakeNotifier::default(),
        );
        assert_eq!(
            second
                .poll(&meta, &tasks, &calendar, dt("2026-09-21T09:49:40"))
                .unwrap()
                .delivered,
            1
        );
        let count: i64 = meta
            .query_row(
                "SELECT COUNT(*) FROM reminder_fires WHERE entity_kind = 'event'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn startup_catches_up_only_relevant_and_summarizes() {
        let (meta, tasks, calendar) = databases();
        tasks
            .execute(
                "INSERT INTO task_lists (id, name) VALUES ('l', '收件箱')",
                [],
            )
            .unwrap();
        for index in 0..21 {
            tasks.execute(
                "INSERT INTO tasks (id, list_id, title, reminder_at) VALUES (?1, 'l', ?2, '2026-09-20T09:00:00')",
                params![format!("t{index}"), format!("任务 {index}")],
            ).unwrap();
        }
        tasks.execute(
            "INSERT INTO tasks (id, list_id, title, status, reminder_at) VALUES ('done', 'l', '已完成', 'done', '2026-09-20T09:00:00')",
            [],
        ).unwrap();
        let event = create_event(
            &calendar,
            "已结束会议",
            None,
            None,
            "2026-09-20T08:00:00",
            "2026-09-20T09:00:00",
            false,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        set_reminders(&calendar, &event.id, &[10]).unwrap();

        let service = ReminderService::new(
            FakeClock(dt("2026-09-20T10:00:00")),
            FakeNotifier::default(),
        );
        let report = service.startup_catch_up(&meta, &tasks, &calendar).unwrap();
        assert_eq!(
            report.delivered, 1,
            "more than 20 reminders collapse to one summary"
        );
        let count: i64 = meta
            .query_row("SELECT COUNT(*) FROM reminder_fires", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            count, 21,
            "completed task and ended event are not caught up"
        );
    }

    #[test]
    fn denied_permission_degrades_to_in_app_notice() {
        let (meta, tasks, calendar) = databases();
        tasks
            .execute(
                "INSERT INTO task_lists (id, name) VALUES ('l', '收件箱')",
                [],
            )
            .unwrap();
        tasks.execute(
            "INSERT INTO tasks (id, list_id, title, reminder_at) VALUES ('t', 'l', '交周报', '2026-09-20T10:00:00')",
            [],
        ).unwrap();
        let service = ReminderService::new(
            FakeClock(dt("2026-09-20T10:00:10")),
            FakeNotifier {
                sent: Mutex::new(Vec::new()),
                denied: true,
            },
        );
        let report = service
            .poll(&meta, &tasks, &calendar, dt("2026-09-20T09:59:40"))
            .unwrap();
        assert_eq!(report.in_app.len(), 1);
    }
}
